# cordis-rs

A [Cordis](https://github.com/cordiverse/cordis)-inspired plugin harness for
Rust: **scoped services, lifecycle-managed cleanup, and a bounded memory
budget**. It is the Rust analogue of the harness pattern DSH builds on —
everything is a plugin, and everything a plugin acquires is freed when its
fiber disposes.

```text
crates/cordis-rs/
├── src/
│   ├── context.rs   Context: root/extend/isolate/intercept, dispose, stats
│   ├── fiber.rs     Fiber lifecycle, effects, disposers, Drop teardown
│   ├── plugin.rs    Plugin trait + plugin_fn function adapter
│   ├── registry.rs  Plugin runtimes, inject scheduling, load/unload
│   ├── service.rs   Services: eager/lazy, scoped, fiber-owned
│   ├── events.rs    Event bus: emit/parallel/serial/bail/waterfall + typed
│   ├── memory.rs    MemoryBudget caps + MemoryStats snapshot
│   ├── logger.rs    Named logger on the tracing `cordis` target
│   └── types.rs     Cleanup, Disposer, ids
├── tests/           lifecycle, services, inject, events, memory, dispose
└── examples/hello.rs
```

## Concept map

| Cordis | cordis-rs |
|---|---|
| `new Context()` | `Context::root()` |
| `ctx.extend()` / `isolate()` / `intercept()` | same names |
| `ctx.plugin(p, config)` → thenable fiber | `ctx.plugin(p, config)` → `Fiber` (`await_ready()`) |
| `Service` class + `ctx.provide` | `ctx.provide` / `provide_lazy` + `use_service::<T>` |
| `ctx.on / emit / parallel / serial / bail / waterfall` | `on_dyn / emit_dyn / parallel_dyn / serial_dyn / bail_dyn / waterfall_dyn` (+ `_t` typed variants) |
| `ctx.effect()` | `ctx.effect()` / `effect_fn()` |
| `fiber.dispose()` | `fiber.dispose()` (async, idempotent) |
| `inject` dependency scheduling | `ctx.inject(&["db"], cb)` — re-runs on service change |

## Quick start

```rust
use cordis_rs::prelude::*;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), CordisError> {
    let root = Context::root();
    root.provide_lazy("counter", |_ctx| Ok(Arc::new(AtomicUsize::new(0))))?;

    let fiber = root.plugin(
        plugin_fn("incrementer", |ctx, _config| async move {
            let counter = ctx.use_service::<AtomicUsize>("counter")?;
            ctx.on_dyn("app/ready", move |_event| {
                counter.fetch_add(1, Ordering::SeqCst);
                async { Flow::Continue }
            })?;
            Ok(())
        }),
        json!({}),
    )?;
    fiber.await_ready().await?;

    root.emit_dyn("app/ready", &json!("started"))?;
    println!("{:?}", root.memory_stats());

    root.dispose().await?; // frees every listener/service/fiber
    Ok(())
}
```

## Memory control

The Rust port adds explicit memory accounting on top of Cordis's dispose
semantics:

- **Deterministic disposal** — every service, listener, and effect is owned
  by a fiber; `dispose()` (or root `dispose()`) frees them in reverse
  registration order.
- **Drop-safe registry** — dropping the root context drops every fiber it
  owns, and `FiberInner::drop` runs sync cleanups, so registry state never
  leaks even without an explicit dispose. Async cleanups need
  `dispose()` to be guaranteed (Drop cannot await).
- **Lazy services** — `provide_lazy` builds the value on first access;
  never-used dependencies are never allocated.
- **Budget caps** — `MemoryBudget` bounds fibers, pending fibers, services,
  listeners, and contexts; exceeding a cap fails with `BUDGET_EXCEEDED`
  instead of growing unbounded.
- **Bounded event history** — a ring buffer of recent events for
  diagnostics, truncated to `max_event_history` (payload bytes tracked).
- **Observability** — `memory_stats()` reports live counts and a byte
  estimate; `for_each_plugin()` lists runtimes with active fiber counts.

## Self-evolution

An evolving agent harness runs a generate → evaluate → select → retire
loop, and `evolution.rs` provides the loop's substrate:

| Evolution mechanism | cordis-rs primitive |
|---|---|
| Generate (agent writes a plugin candidate) | `Evolution::spawn(name, plugin, config, provenance)` — the candidate becomes a fiber with lineage (`Provenance`: source, prompt, parent generation) |
| Dedupe | Same content hash (name + plugin identity + config + source) reuses the live gene instead of allocating again |
| Evaluate | `record_use` / `record_failure` feed a `Fitness` score (`uses − 10·failures − 100·panics − 5·restarts`) |
| Quarantine | A failing candidate restarts up to `max_restarts` times, then stays as a dead gene (evicted first by selection); a panicking plugin becomes `Failed`, never crashes the process |
| Select | `gc()` purges every gene whose score is not positive |
| Retire | The gene pool has a hard `capacity`: spawning beyond it evicts the weakest gene — evolution cannot grow memory unboundedly |
| Persist | `snapshot()` / `genes()` serialize the surviving genome (code itself is stored by the agent, keyed by `hash`); `respawn(record, plugin)` restores a gene with its fitness after a restart |

See `examples/evolution.rs` for a simulated loop; the harness `MemoryBudget`
caps still apply underneath, so generated code can never exhaust the process.

## Design notes

- **Payloads are JSON** (`serde_json::Value`): typed APIs deserialize per
  dispatch, dynamic APIs pass values through — every event is inspectable
  and serializable, unlike `Box<dyn Any>` downcasts.
- **No Arc cycles**: the root context owns its fibers; fiber contexts point
  forward to the shared root state; the root state holds only weak handles
  back. Dropping the root context therefore collects the harness.
- **Fiber states** — `pending → loading → active` (or `failed`), with
  `unloading` and terminal `disposed`; transitions are emitted as
  `internal/status` events. `internal/plugin` and `internal/service`
  mirror Cordis's diagnostics.

## Test report

All suites green on the zode workspace (rust 1.94, macOS). The end-to-end
self-test below exercises the real zode integration layer, not just the
in-crate simulation.

### Suites

| Suite | Command | Result |
|---|---|---|
| cordis-rs (lifecycle, services, inject, events, memory, dispose, evolution, process) | `cargo test -p cordis-rs` | **50 passed** |
| cordis-rs process plugins (child protocol, dispose-kills-child, live binary swap) | `cargo test -p cordis-rs --test process` | **3 passed** |
| cordis-rs evolution (lineage, dedupe, eviction, quarantine, snapshot/respawn, budget) | `cargo test -p cordis-rs --test evolution` | **8 passed** |
| zode-core evolution integration (group fitness, genome restore, unfit groups, disabled mode) | `cargo test -p zode-core --lib evolution::` | **5 passed** |
| zode-core QuickJS gene layer (events + cleanup, live source swap, interrupt deadline, memory cap) | `cargo test -p zode-core --test js_plugin_it` | **4 passed** |
| zode-core full lib suite (evolution wiring included) | `cargo test -p zode-core --lib` | **983 passed** |
| Lint / format | `cargo clippy --all-targets -D warnings` + `cargo fmt --check` | clean |

### End-to-end self-test: generate → evaluate → select → retire

```sh
cargo run -p zode-core --example evolution_self_test
```

The run drives the real `EvolutionHarness` (tool-group fitness from the
hook pipeline + `spawn_js_gene` candidates + capacity selection + genome
persistence) and ends with `SELF-TEST PASSED`. Observed output:

```text
== phase 1: tool-group fitness from the hook pipeline ==
  group git          score  -10
  group shell        score    1
  group filesystem   score    2
  group todo         score  -10
  unfit groups (disable candidates): [("git", -10), ("todo", -10)]
== phase 2: the agent evolves JS candidates (no compiler) ==
  evolving candidate-v1 (usefulness 3)...
  harness observed reply from candidate-v1
  evolving candidate-v2 (usefulness 1)...
  evolving candidate-v3 (usefulness 5)...
== phase 3: pool at capacity 4 — selection fired ==
  gen# 3 filesystem   score  2 live true (parent None)
  gen# 5 candidate-v1 score  3 live true (parent Some(1))
  gen# 6 candidate-v2 score  1 live true (parent Some(1))
  gen# 7 candidate-v3 score  5 live true (parent Some(1))
== phase 4: genome persistence ==
  genome.json (1377 bytes) — restored with fitness on next start
== memory ==
  before dispose: fibers 5, listeners 4, history_records 38
  after dispose:  fibers 0, listeners 0, history_records 0
SELF-TEST PASSED
```

Assertions the self-test enforces: the hook pipeline scores failures as
`-10` and uses as `+1`; `unfit_groups()` names `git` and `todo`; with a
tight capacity of 4, the three weakest genes (`git` → `todo` → `shell`)
are evicted as candidates arrive; the fittest candidate (`candidate-v3`,
score 5) and the most-used group (`filesystem`) survive; the genome
persists with content hashes and fitness; and `dispose()` reclaims every
fiber, listener, and history record.

### Regressions the tests caught (and now pin)

- **Per-fiber plugin instances** — two fibers of the same plugin type with
  different state (e.g. two `ProcessPlugin`s with different binaries) used
  to load the FIRST instance from the shared runtime record; fibers now
  hold their own instance (`live_replacement_swaps_the_binary`).
- **Fiber-owned disposers** — `provide`/`on` disposers that were only
  returned (not registered on the owning fiber) leaked on fiber dispose;
  they are now registered AND returned (`dropping_root_with_live_child`).
- **Ready-handshake deadlock** — the JS worker signalled readiness after
  its command loop instead of before it, deadlocking `apply()`; fixed by
  the ordered handshake (`js_gene_handles_events_and_cleans_up`).
- **Lazy-service accounting** — replacing a lazy service leaked its
  uninitialized counter; stats now stay honest across replacements.
- **Async subscription races** — subprocess/JS genes subscribe
  asynchronously after spawn; tests dispatch in a retry-until-answer loop
  instead of assuming the subscription landed.

## License

MIT
