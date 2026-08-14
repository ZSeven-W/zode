//! Evolution layer: lineage, fitness, dedupe, selection, quarantine.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use cordis_rs::prelude::*;
use serde_json::json;

fn provenance(source: &str, parent: Option<u64>) -> Provenance {
    Provenance {
        source: source.to_string(),
        prompt: Some(format!("make a tool for {source}")),
        parent,
    }
}

fn ok_plugin(name: &'static str) -> impl Plugin {
    plugin_fn(name, |_ctx, _config| async { Ok(()) })
}

#[tokio::test]
async fn spawn_records_lineage_and_fitness() -> Result<(), CordisError> {
    let root = Context::root();
    let evolution = Evolution::attach(&root, EvolutionConfig::default())?;

    let first = evolution
        .spawn(
            "seed",
            Arc::new(ok_plugin("seed")),
            json!({}),
            provenance("agent", None),
        )
        .await?;
    assert_eq!(first.state(), FiberState::Active);
    evolution.record_use(&first);
    evolution.record_use(&first);

    let second = evolution
        .spawn(
            "evolved",
            Arc::new(ok_plugin("evolved")),
            json!({}),
            provenance("agent", Some(1)),
        )
        .await?;
    assert_eq!(second.state(), FiberState::Active);

    let genes = evolution.genes();
    assert_eq!(genes.len(), 2);
    assert_eq!(genes[0].generation, 1);
    assert_eq!(genes[1].generation, 2);
    assert_eq!(genes[1].provenance.parent, Some(1));
    assert!(genes[0].live && genes[1].live);

    assert_eq!(
        evolution.fitness(first.id()),
        Some(Fitness {
            uses: 2,
            ..Fitness::default()
        })
    );
    assert_eq!(evolution.score(first.id()), Some(2));
    assert!(evolution.score(first.id()) > evolution.score(second.id()));
    Ok(())
}

#[tokio::test]
async fn dedupe_reuses_the_live_gene() -> Result<(), CordisError> {
    let root = Context::root();
    let evolution = Evolution::attach(&root, EvolutionConfig::default())?;
    let first = evolution
        .spawn(
            "tool",
            Arc::new(ok_plugin("tool")),
            json!({ "a": 1 }),
            provenance("agent", None),
        )
        .await?;
    let again = evolution
        .spawn(
            "tool",
            Arc::new(ok_plugin("tool")),
            json!({ "a": 1 }),
            provenance("agent", None),
        )
        .await?;
    // Same content hash → same fiber, no second allocation.
    assert_eq!(first.id(), again.id());
    assert_eq!(evolution.len(), 1);
    // A different config is a different gene.
    let other = evolution
        .spawn(
            "tool",
            Arc::new(ok_plugin("tool")),
            json!({ "a": 2 }),
            provenance("agent", None),
        )
        .await?;
    assert_ne!(first.id(), other.id());
    assert_eq!(evolution.len(), 2);
    Ok(())
}

#[tokio::test]
async fn capacity_evicts_the_weakest_gene() -> Result<(), CordisError> {
    let root = Context::root();
    let evolution = Evolution::attach(
        &root,
        EvolutionConfig {
            capacity: 2,
            max_restarts: 0,
        },
    )?;

    let strong = evolution
        .spawn(
            "strong",
            Arc::new(ok_plugin("strong")),
            json!({}),
            provenance("agent", None),
        )
        .await?;
    for _ in 0..5 {
        evolution.record_use(&strong);
    }
    let weak = evolution
        .spawn(
            "weak",
            Arc::new(ok_plugin("weak")),
            json!({}),
            provenance("agent", None),
        )
        .await?;
    evolution.record_use(&weak);

    // Pool is full: the next gene evicts the weakest (weak, score 1).
    let newcomer = evolution
        .spawn(
            "newcomer",
            Arc::new(ok_plugin("newcomer")),
            json!({}),
            provenance("agent", None),
        )
        .await?;
    assert_eq!(evolution.len(), 2);
    let genes = evolution.genes();
    assert!(genes.iter().any(|g| g.name == "strong"));
    assert!(genes.iter().any(|g| g.name == "newcomer"));
    assert!(!genes.iter().any(|g| g.name == "weak"));
    assert_eq!(weak.state(), FiberState::Disposed);
    assert_eq!(newcomer.state(), FiberState::Active);
    Ok(())
}

#[tokio::test]
async fn failing_gene_restarts_then_quarantines() -> Result<(), CordisError> {
    let root = Context::root();
    let attempts = Arc::new(AtomicUsize::new(0));
    let flaky = plugin_fn("flaky", {
        let attempts = attempts.clone();
        move |_ctx, _config| {
            let attempts = attempts.clone();
            async move {
                let n = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                if n < 3 {
                    Err(CordisError::ServiceNotFound("boom".to_string()))
                } else {
                    Ok(())
                }
            }
        }
    });
    let evolution = Evolution::attach(
        &root,
        EvolutionConfig {
            capacity: 4,
            max_restarts: 2,
        },
    )?;
    let fiber = evolution
        .spawn(
            "flaky",
            Arc::new(flaky),
            json!({}),
            provenance("agent", None),
        )
        .await?;
    // Two failures then success: two restarts consumed, gene is live.
    assert_eq!(fiber.state(), FiberState::Active);
    let genes = evolution.genes();
    assert_eq!(genes.len(), 1);
    assert_eq!(genes[0].fitness.restarts, 2);
    assert_eq!(genes[0].fitness.failures, 2);
    assert!(genes[0].live);
    Ok(())
}

#[tokio::test]
async fn always_failing_gene_is_quarantined() -> Result<(), CordisError> {
    let root = Context::root();
    let broken = plugin_fn("broken", |_ctx, _config| async {
        Err(CordisError::ServiceNotFound("always".to_string()))
    });
    let evolution = Evolution::attach(
        &root,
        EvolutionConfig {
            capacity: 4,
            max_restarts: 1,
        },
    )?;
    let fiber = evolution
        .spawn(
            "broken",
            Arc::new(broken),
            json!({}),
            provenance("agent", None),
        )
        .await?;
    assert_eq!(fiber.state(), FiberState::Failed);
    assert!(fiber.error().is_some());
    let genes = evolution.genes();
    assert_eq!(genes.len(), 1);
    assert!(!genes[0].live);
    assert_eq!(genes[0].fitness.failures, 2); // initial + one restart attempt
    assert_eq!(genes[0].fitness.restarts, 1);
    // A quarantined gene has negative score: selection removes it first.
    assert!(genes[0].fitness.score() < 0);
    let evicted = evolution.gc().await;
    assert_eq!(evicted.len(), 1);
    assert_eq!(evolution.len(), 0);
    Ok(())
}

#[tokio::test]
async fn gc_purges_unfit_and_keeps_used_genes() -> Result<(), CordisError> {
    let root = Context::root();
    let evolution = Evolution::attach(&root, EvolutionConfig::default())?;
    let useful = evolution
        .spawn(
            "useful",
            Arc::new(ok_plugin("useful")),
            json!({}),
            provenance("agent", None),
        )
        .await?;
    evolution.record_use(&useful);
    evolution
        .spawn(
            "unused",
            Arc::new(ok_plugin("unused")),
            json!({}),
            provenance("agent", None),
        )
        .await?;
    evolution
        .spawn(
            "failing",
            Arc::new(plugin_fn("failing", |_ctx, _config| async {
                Err(CordisError::ServiceNotFound("x".to_string()))
            })),
            json!({}),
            provenance("agent", None),
        )
        .await?;

    let evicted = evolution.gc().await;
    let evicted_names: Vec<&str> = evicted.iter().map(|g| g.name.as_str()).collect();
    assert!(evicted_names.contains(&"unused"));
    assert!(evicted_names.contains(&"failing"));
    assert_eq!(evolution.len(), 1);
    assert_eq!(evolution.genes()[0].name, "useful");
    Ok(())
}

#[tokio::test]
async fn snapshot_and_respawn_carry_fitness() -> Result<(), CordisError> {
    let root = Context::root();
    let evolution = Evolution::attach(&root, EvolutionConfig::default())?;
    let fiber = evolution
        .spawn(
            "gene",
            Arc::new(ok_plugin("gene")),
            json!({ "n": 7 }),
            provenance("agent", None),
        )
        .await?;
    evolution.record_use(&fiber);
    evolution.record_use(&fiber);

    // Persist the genome, then tear the pool down (simulated restart).
    let snapshot = evolution.snapshot();
    let records: Vec<GeneRecord> = serde_json::from_value(snapshot).unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].fitness.uses, 2);
    evolution.gc_to(0).await;
    assert_eq!(evolution.len(), 0);
    assert_eq!(root.memory_stats().fibers, 1); // only the root fiber remains

    // Respawn from the record: fitness carries over.
    let restored = evolution
        .respawn(&records[0], Arc::new(ok_plugin("gene")))
        .await?;
    assert_eq!(restored.state(), FiberState::Active);
    assert_eq!(evolution.genes()[0].fitness.uses, 2);
    assert_eq!(evolution.genes()[0].config["n"], json!(7));
    Ok(())
}

#[tokio::test]
async fn evolution_respects_harness_memory_budget() -> Result<(), CordisError> {
    let root = Context::root();
    // Tight harness budget: only root + one gene fiber fit, so the second
    // gene is rejected even though the gene pool allows it.
    root.set_budget(MemoryBudget {
        max_fibers: 2,
        ..Default::default()
    });
    let evolution = Evolution::attach(&root, EvolutionConfig::default())?;
    evolution
        .spawn(
            "a",
            Arc::new(ok_plugin("a")),
            json!({}),
            provenance("agent", None),
        )
        .await?;
    let err = evolution
        .spawn(
            "b",
            Arc::new(ok_plugin("b")),
            json!({}),
            provenance("agent", None),
        )
        .await
        .unwrap_err();
    assert_eq!(err.code(), "BUDGET_EXCEEDED");
    Ok(())
}
