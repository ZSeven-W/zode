//! A simulated self-evolution loop on top of cordis-rs.
//!
//! Each round: the "agent" produces candidate plugins; the harness installs
//! them as genes inside a bounded pool, the runtime probes them and reports
//! fitness, and selection retires the weakest. Quarantined genes are kept
//! for lineage but evicted first.
//!
//! Run with: `cargo run -p cordis-rs --example evolution`

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use cordis_rs::prelude::*;
use serde_json::json;

fn candidate(name: &'static str, usefulness: usize) -> impl Plugin {
    plugin_fn(name, move |ctx, _config| async move {
        // A real gene would register tools/hooks here. The probe below
        // measures "usefulness" via events this gene answers.
        let uses = Arc::new(AtomicUsize::new(0));
        ctx.provide_lazy("probe", move |_| Ok(Arc::new(usefulness)))?;
        ctx.on_dyn("probe/run", move |_event| {
            uses.fetch_add(1, Ordering::SeqCst);
            async { Flow::Continue }
        })?;
        Ok(())
    })
}

fn provenance(source: &str, parent: Option<u64>) -> Provenance {
    Provenance {
        source: source.to_string(),
        prompt: Some(format!("candidate from {source}")),
        parent,
    }
}

#[tokio::main]
async fn main() -> Result<(), CordisError> {
    let root = Context::root();
    // The gene pool is the memory bound of evolution: at most 3 candidates
    // may be alive at once; the weakest is evicted to make room.
    let evolution = Evolution::attach(
        &root,
        EvolutionConfig {
            capacity: 3,
            max_restarts: 1,
        },
    )?;

    println!("== generation 1: seed candidate");
    let _seed = evolution
        .spawn(
            "seed",
            Arc::new(candidate("seed", 1)),
            json!({}),
            provenance("builtin", None),
        )
        .await?;

    println!("== generation 2: the agent evolves three candidates from the seed");
    for (name, usefulness) in [("candidate-a", 3), ("candidate-b", 2), ("candidate-c", 5)] {
        let gene = evolution
            .spawn(
                name,
                Arc::new(candidate(name, usefulness)),
                json!({}),
                provenance("agent:sonnet", Some(1)),
            )
            .await?;
        // Probe: a successful call is one observed use of the gene.
        root.parallel_dyn("probe/run", &json!({})).await?;
        for _ in 0..usefulness {
            evolution.record_use(&gene);
        }
    }

    let seed_alive = evolution.genes().iter().any(|g| g.name == "seed");
    println!(
        "== pool at capacity {}: seed {} (weakest gene is replaced when the pool fills)",
        evolution.capacity(),
        if seed_alive { "kept" } else { "replaced" },
    );

    println!("== selection: purge genes that never demonstrated value");
    let evicted = evolution.gc().await;
    println!(
        "evicted: {:?}",
        evicted.iter().map(|g| g.name.as_str()).collect::<Vec<_>>()
    );

    println!("== genome snapshot (surviving genes)");
    for gene in evolution.genes() {
        println!(
            "  gen#{} {} (parent {:?}, score {}, live {})",
            gene.generation,
            gene.name,
            gene.provenance.parent,
            gene.fitness.score(),
            gene.live,
        );
    }
    let snapshot = evolution.snapshot();
    println!("snapshot bytes: {}", snapshot.to_string().len());

    println!("== memory");
    println!(
        "  pool: {} genes / {} capacity",
        evolution.len(),
        evolution.capacity()
    );
    println!("  {:?}", root.memory_stats());

    root.dispose().await?;
    println!("  after dispose: {:?}", root.memory_stats());
    Ok(())
}
