//! End-to-end self-evolution self-test: the full generate → evaluate →
//! select → retire loop over the real zode integration layer.
//!
//!   phase 1  tool-group fitness from the hook pipeline (simulated tool results)
//!   phase 2  the agent evolves JS candidates (no compiler needed)
//!   phase 3  capacity pressure: the weakest gene is evicted (selection)
//!   phase 4  genome persistence (fitness carried across restarts)
//!
//! Run with: cargo run -p zode-core --example evolution_self_test

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use cordis_rs::prelude::*;
use serde_json::json;
use zode_core::evolution::{EvolutionHarness, EvolutionSettings};

fn js_candidate(candidate: &str) -> String {
    format!(
        r#"(function () {{
  return {{
    apply: function (host) {{
      host.on("probe", function (payload) {{
        host.emit("gene/result", JSON.stringify({{ candidate: "{candidate}" }}));
        return null;
      }});
    }},
  }};
}})"#
    )
}

async fn wait_until(predicate: impl Fn() -> bool, what: &str) {
    for _ in 0..200 {
        if predicate() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    eprintln!("WARN: timed out waiting for {what}");
}

#[tokio::main]
async fn main() -> Result<(), CordisError> {
    let dir = std::env::temp_dir().join(format!("zode-evolution-self-test-{}", std::process::id()));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();

    let harness = EvolutionHarness::init(
        dir.clone(),
        EvolutionSettings {
            capacity: 4, // tight on purpose: selection must fire
            ..Default::default()
        },
    );
    let ctx = harness.context();

    println!("== phase 1: tool-group fitness from the hook pipeline ==");
    for _ in 0..2 {
        harness.record_tool_result("FileRead", true);
    }
    harness.record_tool_result("Bash", true);
    harness.record_tool_result("TodoWrite", false);
    harness.record_tool_result("GitStatus", false);
    wait_until(|| harness.genes().len() >= 4, "tool-group genes").await;
    for gene in harness.genes() {
        println!("  group {:12} score {:>4}", gene.name, gene.fitness.score());
    }
    println!(
        "  unfit groups (disable candidates): {:?}",
        harness.unfit_groups()
    );

    println!("== phase 2: the agent evolves JS candidates (no compiler) ==");
    let replies = Arc::new(AtomicUsize::new(0));
    ctx.on_dyn("gene/result", {
        let replies = replies.clone();
        move |event| {
            let candidate = event
                .payload
                .get("candidate")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();
            println!("  harness observed reply from {candidate}");
            replies.fetch_add(1, Ordering::SeqCst);
            async { Flow::Continue }
        }
    })?;

    let mut candidates: Vec<(String, Fiber)> = Vec::new();
    for (candidate, usefulness) in [
        ("candidate-v1", 3usize),
        ("candidate-v2", 1),
        ("candidate-v3", 5),
    ] {
        println!("  evolving {candidate} (usefulness {usefulness})...");
        let fiber = harness
            .spawn_js_gene(
                candidate,
                js_candidate(candidate),
                json!({}),
                Provenance {
                    source: "agent:sonnet".to_string(),
                    prompt: Some(format!("build {candidate}")),
                    parent: Some(1),
                },
            )
            .await?;
        // Probe: dispatch until the gene answers, then score its usefulness.
        let before = replies.load(Ordering::SeqCst);
        for _ in 0..100 {
            let _ = ctx.parallel_dyn("probe", &json!({})).await;
            tokio::time::sleep(Duration::from_millis(5)).await;
            if replies.load(Ordering::SeqCst) > before {
                break;
            }
        }
        for _ in 0..usefulness {
            harness.record_gene_use(&fiber);
        }
        candidates.push((candidate.to_string(), fiber));
    }

    println!(
        "== phase 3: pool at capacity {} — selection fired ==",
        harness.capacity()
    );
    for gene in harness.genes() {
        println!(
            "  gen#{:>2} {:16} score {:>4} live {} (parent {:?})",
            gene.generation,
            gene.name,
            gene.fitness.score(),
            gene.live,
            gene.provenance.parent,
        );
    }
    // With capacity 4 and 4 group genes + 3 candidates, the three weakest
    // genes were evicted along the way; only the fittest survive.
    assert_eq!(harness.genes().len(), 4, "pool must stay within capacity");
    let names: Vec<String> = harness.genes().iter().map(|g| g.name.clone()).collect();
    assert!(
        names.contains(&"candidate-v3".to_string()),
        "fittest candidate must survive"
    );
    assert!(
        names.contains(&"filesystem".to_string()),
        "most-used group must survive"
    );

    println!("== phase 4: genome persistence ==");
    harness.persist();
    let genome_path = dir.join("evolution").join("genome.json");
    let genome = std::fs::read_to_string(&genome_path).unwrap_or_default();
    println!(
        "  genome.json ({} bytes) — restored with fitness on next start:",
        genome.len()
    );
    for record in harness.snapshot() {
        println!(
            "    {:16} hash {:016x} score {:>4}",
            record.name,
            record.hash,
            record.fitness.score()
        );
    }

    println!("== memory ==");
    println!("  before dispose: {:?}", ctx.memory_stats());
    ctx.dispose().await?;
    println!("  after dispose:  {:?}", ctx.memory_stats());
    let _ = candidates;
    println!("SELF-TEST PASSED");
    Ok(())
}
