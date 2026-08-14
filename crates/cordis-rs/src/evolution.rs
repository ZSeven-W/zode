//! Self-evolution support: generated plugin lineages with fitness-based
//! selection and a bounded gene pool.
//!
//! An evolving agent harness runs a generate → evaluate → select → retire
//! loop: the agent writes plugin candidates, the harness loads each one as
//! a fiber, the runtime reports how useful it was, and selection keeps the
//! fittest genes inside a hard memory cap. The safety substrate is the rest
//! of this crate: candidates run under `MemoryBudget` caps, a panicking
//! candidate becomes a `Failed` fiber instead of crashing the process, and
//! `dispose()` reclaims everything a retired gene acquired.
//!
//! Code is intentionally NOT stored in a `GeneRecord`: the agent persists
//! generated code itself (keyed by `hash`), while the snapshot records
//! *which* genes survived selection and with what fitness.

use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::context::Context;
use crate::error::CordisError;
use crate::fiber::{Fiber, FiberId, FiberInner, FiberState};
use crate::plugin::Plugin;

/// Provenance of a generated plugin — the "genome" of a self-evolving
/// harness. A generation is one candidate produced by an evolution step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// Who produced this generation (agent model id, user, builtin seed).
    pub source: String,
    /// The evolution prompt/spec that produced it (optional).
    pub prompt: Option<String>,
    /// The generation this one evolved from (`None` = seed generation).
    pub parent: Option<u64>,
}

/// Fitness signals accumulated over a gene's lifetime. The agent runtime
/// reports uses and failures; the harness counts restarts and panics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fitness {
    /// Successful uses observed by the runtime (e.g. useful tool calls).
    pub uses: u64,
    /// Runtime failures reported for this gene.
    pub failures: u64,
    /// Startup panics observed across restarts.
    pub panics: u64,
    /// Load restarts consumed (failure budget spent).
    pub restarts: u64,
}

impl Fitness {
    /// Selection score: usage minus weighted penalties. Panics are the most
    /// heavily penalized, then failures, then restarts.
    pub fn score(&self) -> i64 {
        self.uses as i64
            - 10 * self.failures as i64
            - 100 * self.panics as i64
            - 5 * self.restarts as i64
    }
}

/// A surviving (or quarantined) generation, serializable as the harness's
/// genome snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeneRecord {
    /// Generation number, assigned by the evolution layer.
    pub generation: u64,
    /// Gene name (usually the plugin name).
    pub name: String,
    /// Lineage and origin of this gene.
    pub provenance: Provenance,
    /// Content hash (name + plugin identity + config + source); used for
    /// dedupe and as the code key the agent persists generated code under.
    pub hash: u64,
    /// The config the gene was loaded with (respawn input).
    pub config: Value,
    /// Accumulated fitness.
    pub fitness: Fitness,
    /// Whether the gene is currently loaded and active.
    pub live: bool,
}

struct Gene {
    record: GeneRecord,
    fiber: Option<Weak<FiberInner>>,
}

impl Gene {
    fn fiber(&self) -> Option<Arc<FiberInner>> {
        self.fiber.as_ref().and_then(|weak| weak.upgrade())
    }

    fn alive(&self) -> bool {
        self.fiber()
            .map(|fiber| fiber.state() == FiberState::Active)
            .unwrap_or(false)
    }
}

/// Bounds for the evolution layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvolutionConfig {
    /// Maximum number of genes retained in the pool. Spawning beyond it
    /// evicts the weakest gene first — the memory bound of evolution.
    pub capacity: usize,
    /// Restart budget per generation before quarantine.
    pub max_restarts: u32,
}

impl Default for EvolutionConfig {
    fn default() -> Self {
        EvolutionConfig {
            capacity: 32,
            max_restarts: 2,
        }
    }
}

fn content_hash(name: &str, plugin: &Arc<dyn Plugin>, config: &Value, source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    name.hash(&mut hasher);
    plugin.name().hash(&mut hasher);
    plugin.runtime_type().hash(&mut hasher);
    if let Some(content_id) = plugin.content_id() {
        content_id.hash(&mut hasher);
    }
    config.to_string().hash(&mut hasher);
    source.hash(&mut hasher);
    hasher.finish()
}

/// The evolution layer of a harness: bounded gene pool with lineage,
/// fitness, dedupe, quarantine, and selection.
pub struct Evolution {
    ctx: Context,
    config: EvolutionConfig,
    next_gen: AtomicU64,
    genes: Mutex<BTreeMap<u64, Gene>>,
    by_fiber: Mutex<HashMap<FiberId, u64>>,
}

impl Evolution {
    /// Attach an evolution layer to a context.
    pub fn attach(ctx: &Context, config: EvolutionConfig) -> Result<Arc<Self>, CordisError> {
        ctx.check_alive()?;
        Ok(Arc::new(Evolution {
            ctx: ctx.clone(),
            config,
            next_gen: AtomicU64::new(1),
            genes: Mutex::new(BTreeMap::new()),
            by_fiber: Mutex::new(HashMap::new()),
        }))
    }

    pub fn capacity(&self) -> usize {
        self.config.capacity
    }

    /// Number of genes currently in the pool (live + quarantined).
    pub fn len(&self) -> usize {
        self.genes.lock().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The harness context the gene pool lives on — dispatch events here to
    /// reach gene listeners and observe gene emissions.
    pub fn context(&self) -> Context {
        self.ctx.clone()
    }

    /// Spawn a generated plugin as a new gene.
    ///
    /// - Duplicates (same content hash, live gene) return the existing fiber
    ///   instead of allocating a second one.
    /// - At capacity, the weakest gene is evicted first (evolve = replace).
    /// - A failing candidate restarts up to `max_restarts` times; when the
    ///   budget is exhausted it is quarantined (kept as a dead gene for
    ///   lineage and snapshots, evicted first by selection).
    ///
    /// Returns the final fiber — `Active` on success, `Failed` when the
    /// candidate was quarantined (see `Fiber::error`).
    pub async fn spawn(
        &self,
        name: &'static str,
        plugin: Arc<dyn Plugin>,
        config: Value,
        provenance: Provenance,
    ) -> Result<Fiber, CordisError> {
        self.ctx.check_alive()?;
        let hash = content_hash(name, &plugin, &config, &provenance.source);

        // Dedupe: the same gene already live? Reuse it (saves memory).
        if let Some(existing) = self
            .genes
            .lock()
            .unwrap()
            .values()
            .find(|gene| gene.record.hash == hash && gene.alive())
            .and_then(Gene::fiber)
        {
            return Ok(Fiber { inner: existing });
        }

        self.make_room().await;
        let generation = self.next_gen.fetch_add(1, Ordering::SeqCst);

        let mut restarts: u64 = 0;
        let mut failures: u64 = 0;
        let mut panics: u64 = 0;
        let mut fiber = self.ctx.plugin_dyn(plugin.clone(), config.clone())?;
        loop {
            match fiber.await_ready().await {
                Ok(()) => break,
                Err(_) => {
                    failures += 1;
                    if fiber
                        .error()
                        .map(|error| error.contains("panicked"))
                        .unwrap_or(false)
                    {
                        panics += 1;
                    }
                    if restarts < self.config.max_restarts as u64 {
                        restarts += 1;
                        fiber.dispose().await;
                        fiber = self.ctx.plugin_dyn(plugin.clone(), config.clone())?;
                    } else {
                        break; // quarantine
                    }
                }
            }
        }

        let live = fiber.state() == FiberState::Active;
        let fitness = Fitness {
            uses: 0,
            failures,
            panics,
            restarts,
        };
        let weak = Arc::downgrade(&fiber.inner);
        let record = GeneRecord {
            generation,
            name: name.to_string(),
            provenance,
            hash,
            config,
            fitness,
            live,
        };
        self.genes.lock().unwrap().insert(
            generation,
            Gene {
                record,
                fiber: Some(weak),
            },
        );
        self.by_fiber.lock().unwrap().insert(fiber.id(), generation);
        Ok(fiber)
    }

    /// Restore a gene from a snapshot record (e.g. after a restart). The
    /// carried fitness is preserved so selection continues where it left
    /// off. The caller supplies the plugin code matching `record.hash`.
    pub async fn respawn(
        &self,
        record: &GeneRecord,
        plugin: Arc<dyn Plugin>,
    ) -> Result<Fiber, CordisError> {
        self.ctx.check_alive()?;
        self.make_room().await;
        // The supplied plugin must match the recorded content — restoring a
        // gene with different code would silently misattribute fitness.
        let actual = content_hash(
            &record.name,
            &plugin,
            &record.config,
            &record.provenance.source,
        );
        if actual != record.hash {
            return Err(CordisError::ConfigInvalid(
                record.name.clone(),
                format!(
                    "content mismatch: recorded hash {} != supplied {}",
                    record.hash, actual
                ),
            ));
        }
        // Future spawns must not collide with the restored generation number.
        self.next_gen
            .fetch_max(record.generation + 1, Ordering::SeqCst);
        let fiber = self.ctx.plugin_dyn(plugin, record.config.clone())?;
        let mut record = record.clone();
        record.live = fiber.await_ready().await.is_ok();
        if !record.live {
            record.fitness.failures += 1;
        }
        let generation = record.generation;
        let weak = Arc::downgrade(&fiber.inner);
        self.genes.lock().unwrap().insert(
            generation,
            Gene {
                record,
                fiber: Some(weak),
            },
        );
        self.by_fiber.lock().unwrap().insert(fiber.id(), generation);
        Ok(fiber)
    }

    /// Report a successful use of a gene (e.g. a tool call that helped).
    pub fn record_use(&self, fiber: &Fiber) {
        self.adjust(fiber.id(), |fitness| fitness.uses += 1);
    }

    /// Report a runtime failure attributed to a gene.
    pub fn record_failure(&self, fiber: &Fiber) {
        self.adjust(fiber.id(), |fitness| fitness.failures += 1);
    }

    fn adjust(&self, fiber_id: FiberId, f: impl FnOnce(&mut Fitness)) {
        let generation = self.by_fiber.lock().unwrap().get(&fiber_id).copied();
        if let Some(generation) = generation {
            let mut genes = self.genes.lock().unwrap();
            if let Some(gene) = genes.get_mut(&generation) {
                f(&mut gene.record.fitness);
            }
        }
    }

    /// Fitness of a fiber's gene.
    pub fn fitness(&self, fiber_id: FiberId) -> Option<Fitness> {
        let generation = self.by_fiber.lock().unwrap().get(&fiber_id).copied()?;
        self.genes
            .lock()
            .unwrap()
            .get(&generation)
            .map(|gene| gene.record.fitness)
    }

    /// Selection score of a fiber's gene.
    pub fn score(&self, fiber_id: FiberId) -> Option<i64> {
        self.fitness(fiber_id).map(|fitness| fitness.score())
    }

    /// Snapshot of every gene in the pool, oldest generation first.
    pub fn genes(&self) -> Vec<GeneRecord> {
        self.genes
            .lock()
            .unwrap()
            .values()
            .map(|gene| gene.record.clone())
            .collect()
    }

    /// The pool as JSON (for the agent to persist across restarts).
    pub fn snapshot(&self) -> Value {
        serde_json::to_value(self.genes()).unwrap_or(Value::Null)
    }

    /// Evict the weakest genes until the pool holds at most `target`.
    /// Returns the evicted records (their fibers are disposed).
    pub async fn gc_to(&self, target: usize) -> Vec<GeneRecord> {
        let mut evicted = Vec::new();
        loop {
            let victim = {
                let genes = self.genes.lock().unwrap();
                if genes.len() <= target {
                    None
                } else {
                    genes
                        .iter()
                        .min_by_key(|(_, gene)| gene.record.fitness.score())
                        .map(|(generation, _)| *generation)
                }
            };
            match victim {
                Some(generation) => {
                    if let Some(record) = self.evict_gene(generation).await {
                        evicted.push(record);
                    }
                }
                None => break,
            }
        }
        evicted
    }

    /// Selection: purge every gene whose score is not positive (unused,
    /// failing, or quarantined candidates). Returns the evicted records.
    pub async fn gc(&self) -> Vec<GeneRecord> {
        let victims: Vec<u64> = self
            .genes
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, gene)| gene.record.fitness.score() <= 0)
            .map(|(generation, _)| *generation)
            .collect();
        let mut evicted = Vec::new();
        for generation in victims {
            if let Some(record) = self.evict_gene(generation).await {
                evicted.push(record);
            }
        }
        evicted
    }

    async fn make_room(&self) {
        let victim = {
            let genes = self.genes.lock().unwrap();
            if genes.len() < self.config.capacity {
                None
            } else {
                genes
                    .iter()
                    .min_by_key(|(_, gene)| gene.record.fitness.score())
                    .map(|(generation, _)| *generation)
            }
        };
        if let Some(generation) = victim {
            self.evict_gene(generation).await;
        }
    }

    async fn evict_gene(&self, generation: u64) -> Option<GeneRecord> {
        let gene = self.genes.lock().unwrap().remove(&generation)?;
        if let Some(fiber) = gene.fiber() {
            crate::registry::dispose_fiber(&fiber).await;
            self.by_fiber.lock().unwrap().remove(&fiber.id);
        }
        Some(gene.record)
    }
}
