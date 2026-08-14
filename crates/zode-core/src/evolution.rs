//! Self-evolving harness: observes tool-group fitness through the agent
//! hook pipeline, holds each group as a gene in a bounded pool, and persists
//! the genome across restarts.
//!
//! The loop: every 'AfterToolUse' / 'PostToolUseFailure' hook event is
//! attributed to its tool group (the same grouping '/plugin' uses), each
//! group is a gene in the cordis-rs evolution layer, and the fitness score
//! (uses - 10*failures - 100*panics - 5*restarts) drives selection —
//! 'unfit_groups()' names the groups the plugin manager should consider
//! disabling. The genome persists to '<config-dir>/evolution/genome.json'
//! (debounced + on drop) and is restored with its fitness on restart.
//!
//! Genes are tool groups today; the same pool later hosts generated genes
//! (skills, plugin packages, agent-written tools) — 'capacity' is the
//! memory bound on the whole population.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use agent::hook::{HookEvent, HookOutcome, RustHookHandler};
use cordis_rs::{
    plugin_fn, Context, CordisError, Evolution, EvolutionConfig, Fiber, FiberState, GeneRecord,
    Plugin, Provenance,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::config::ConfigManager;
use crate::plugin::{group_of, TOOL_GROUPS};
use crate::sessions::journal::write_json_atomic;

/// Persist the genome every N observed tool results (plus on drop).
const PERSIST_EVERY: u64 = 32;

/// Provenance source shared by every tool-group gene (stable so content
/// hashes match across restarts, which makes spawn dedupe reuse restored
/// genes).
const GROUP_SOURCE: &str = "zode:tool-group";

/// The 'evolution' config block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EvolutionSettings {
    /// Observe tool results and maintain the genome (default true).
    pub enabled: bool,
    /// Gene-pool capacity. Must exceed the tool-group count (11) for full
    /// coverage; headroom is for future generated genes.
    pub capacity: usize,
    /// Restart budget per gene before quarantine.
    pub max_restarts: u32,
}

impl Default for EvolutionSettings {
    fn default() -> Self {
        EvolutionSettings {
            enabled: true,
            capacity: 64,
            max_restarts: 2,
        }
    }
}

static GLOBAL: Mutex<Option<Arc<EvolutionHarness>>> = Mutex::new(None);

/// Process-wide self-evolving harness (one genome per zode process).
pub struct EvolutionHarness {
    // The Arc<Evolution> owns the cordis context (its root state stays alive
    // as long as the harness does), so no separate field is needed.
    evolution: Arc<Evolution>,
    settings: EvolutionSettings,
    genome_path: PathBuf,
    /// One lazily-filled fiber slot per tool group.
    slots: Mutex<HashMap<&'static str, Arc<Mutex<Option<Fiber>>>>>,
    /// Restore + first-spawn serialization (prevents duplicate genes when
    /// several first-touch events race).
    restore_lock: tokio::sync::Mutex<()>,
    restored: AtomicBool,
    records_since_persist: AtomicU64,
}

fn gene_marker() -> Arc<dyn Plugin> {
    // A gene occupies a slot in the harness; the capability itself (the tool
    // group) is registered by the normal engine assembly. The marker gives
    // the gene a fiber, a lifecycle, and a fitness ledger. Its identity is
    // stable (same type + name + config + source), so spawn dedupe reuses
    // restored genes instead of duplicating them.
    Arc::new(plugin_fn("zode-gene-marker", |_ctx, _config| async {
        Ok(())
    }))
}

fn provenance() -> Provenance {
    Provenance {
        source: GROUP_SOURCE.to_string(),
        prompt: Some("built-in tool group".to_string()),
        parent: None,
    }
}

impl EvolutionHarness {
    /// Initialize the process-wide harness. Idempotent: subsequent calls
    /// return the existing instance.
    pub fn init(config_dir: PathBuf, settings: EvolutionSettings) -> Arc<Self> {
        let mut slot = GLOBAL.lock().unwrap();
        if let Some(existing) = slot.as_ref() {
            return existing.clone();
        }
        let harness = Arc::new(Self::build(config_dir, settings));
        *slot = Some(harness.clone());
        harness
    }

    /// Initialize from zode config (no-op when 'evolution.enabled' is
    /// false). Called from engine-template construction so every entry
    /// path (TUI, CLI, extension daemon) shares one genome.
    pub fn init_from_config(cfg: &crate::config::ZodeConfig) {
        if !cfg.evolution.enabled {
            return;
        }
        if let Ok(dir) = ConfigManager::config_dir() {
            Self::init(dir, cfg.evolution.clone());
        }
    }

    /// The process-wide harness, if initialized.
    pub fn global() -> Option<Arc<Self>> {
        GLOBAL.lock().unwrap().clone()
    }

    fn build(config_dir: PathBuf, settings: EvolutionSettings) -> Self {
        let genome_path = config_dir.join("evolution").join("genome.json");
        let context = Context::root();
        let evolution = Evolution::attach(
            &context,
            EvolutionConfig {
                capacity: settings.capacity,
                max_restarts: settings.max_restarts,
            },
        )
        .expect("fresh cordis context always attaches");
        let slots: HashMap<&'static str, Arc<Mutex<Option<Fiber>>>> = TOOL_GROUPS
            .iter()
            .map(|(group, _, _)| (*group, Arc::new(Mutex::new(None))))
            .collect();
        EvolutionHarness {
            evolution,
            settings,
            genome_path,
            slots: Mutex::new(slots),
            restore_lock: tokio::sync::Mutex::new(()),
            restored: AtomicBool::new(false),
            records_since_persist: AtomicU64::new(0),
        }
    }

    pub fn enabled(&self) -> bool {
        self.settings.enabled
    }

    /// Handler for the agent hook runner: attributes tool results to their
    /// tool-group genes.
    pub fn hook(&self) -> RustHookHandler {
        RustHookHandler::new("zode-evolution", move |event| {
            let Some(harness) = Self::global() else {
                return HookOutcome::Ok;
            };
            match event {
                HookEvent::AfterToolUse { tool, ok, .. } => {
                    harness.record_tool_result(tool, *ok);
                }
                HookEvent::PostToolUseFailure { tool, .. } => {
                    harness.record_tool_result(tool, false);
                }
                _ => {}
            }
            HookOutcome::Ok
        })
    }

    /// Attribute one observed tool result to its tool-group gene. Sync:
    /// the fast path records immediately; the first touch of a group
    /// spawns its gene (requires a tokio runtime) and records after.
    pub fn record_tool_result(&self, tool: &str, ok: bool) {
        if !self.enabled() {
            return;
        }
        let Some(group) = group_of(tool) else {
            return;
        };
        let Some(slot) = self.slots.lock().unwrap().get(group).cloned() else {
            return;
        };

        {
            let mut guard = slot.lock().unwrap();
            match guard.as_ref() {
                Some(fiber) if fiber.state() == FiberState::Active => {
                    let fiber = fiber.clone();
                    drop(guard);
                    self.apply(&fiber, ok);
                    return;
                }
                Some(_) => {
                    // Stale slot (gene evicted by selection): refresh lazily.
                    *guard = None;
                }
                None => {}
            }
        }

        // Slow path: ensure the gene exists, then record.
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let Some(harness) = Self::global() else {
            return;
        };
        handle.spawn(async move {
            let fiber = harness.ensure_gene(group).await;
            if let Some(fiber) = fiber {
                *slot.lock().unwrap() = Some(fiber.clone());
                harness.apply(&fiber, ok);
            }
        });
    }

    /// Ensure a live gene for 'group' (spawns or reuses one), restoring the
    /// persisted genome first.
    async fn ensure_gene(&self, group: &'static str) -> Option<Fiber> {
        let _guard = self.restore_lock.lock().await;
        self.restore_once_locked().await;
        match self
            .evolution
            .spawn(group, gene_marker(), json!({}), provenance())
            .await
        {
            Ok(fiber) => Some(fiber),
            Err(err) => {
                tracing::warn!(gene = %group, error = %err, "failed to spawn tool-group gene");
                None
            }
        }
    }

    /// Gene-pool capacity (the memory bound of evolution).
    pub fn capacity(&self) -> usize {
        self.settings.capacity
    }

    /// The harness context: dispatch events here to reach gene listeners
    /// and observe gene emissions.
    pub fn context(&self) -> Context {
        self.evolution.context()
    }

    /// Report a successful use of a generated gene (a tool call that helped).
    pub fn record_gene_use(&self, fiber: &Fiber) {
        self.evolution.record_use(fiber);
    }

    /// Report a runtime failure attributed to a generated gene.
    pub fn record_gene_failure(&self, fiber: &Fiber) {
        self.evolution.record_failure(fiber);
    }

    /// Spawn an agent-generated JavaScript gene — the "evolved" layer.
    /// No compiler is required: the artifact is the source text itself, and
    /// replacing a gene means spawning the new source over the old fiber.
    pub async fn spawn_js_gene(
        &self,
        name: &'static str,
        source: String,
        config: serde_json::Value,
        provenance: Provenance,
    ) -> Result<Fiber, CordisError> {
        let plugin = Arc::new(crate::js_plugin::JsPlugin::new(name, source));
        self.evolution.spawn(name, plugin, config, provenance).await
    }

    /// Respawn genes persisted in the previous run, carrying their fitness.
    async fn restore_once_locked(&self) {
        if self.restored.swap(true, Ordering::SeqCst) {
            return;
        }
        for record in self.load_genome_records() {
            // Only tool-group genes can be restored with the marker plugin;
            // generated JS genes require their source (stored by the agent,
            // keyed by the record's content hash) and are skipped here.
            match self.evolution.respawn(&record, gene_marker()).await {
                Ok(_) => {}
                Err(err) => {
                    tracing::debug!(gene = %record.name, error = %err, "skipping non-marker gene at restore");
                }
            }
        }
    }

    fn apply(&self, fiber: &Fiber, ok: bool) {
        if ok {
            self.evolution.record_use(fiber);
        } else {
            self.evolution.record_failure(fiber);
        }
        if self.records_since_persist.fetch_add(1, Ordering::SeqCst) + 1 >= PERSIST_EVERY {
            self.records_since_persist.store(0, Ordering::SeqCst);
            self.persist();
        }
    }

    /// All genes currently in the pool (live + quarantined).
    pub fn genes(&self) -> Vec<GeneRecord> {
        self.evolution.genes()
    }

    /// The genome view persisted to disk: pool genes merged over records
    /// restored from the previous run (live pool wins on name clashes).
    pub fn snapshot(&self) -> Vec<GeneRecord> {
        let mut merged = self.load_genome_records();
        let live = self.evolution.genes();
        merged.retain(|old| !live.iter().any(|gene| gene.name == old.name));
        merged.extend(live);
        merged
    }

    /// Tool groups whose gene has a non-positive selection score —
    /// candidates for disabling via the plugin manager.
    pub fn unfit_groups(&self) -> Vec<(String, i64)> {
        self.genes()
            .into_iter()
            .filter(|gene| gene.fitness.score() <= 0)
            .map(|gene| (gene.name.clone(), gene.fitness.score()))
            .collect()
    }

    /// Write the genome snapshot atomically. Best-effort: failures only log.
    pub fn persist(&self) {
        if !self.enabled() {
            return;
        }
        if let Err(err) = write_json_atomic(&self.genome_path, &self.snapshot()) {
            tracing::warn!(path = %self.genome_path.display(), error = %err, "failed to persist evolution genome");
        }
    }

    fn load_genome_records(&self) -> Vec<GeneRecord> {
        match std::fs::read_to_string(&self.genome_path) {
            Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }
}

impl Drop for EvolutionHarness {
    fn drop(&mut self) {
        // Last-chance persist (sync): the debounced path already covers
        // steady state; this catches shutdown after a short session.
        self.persist();
    }
}

/// Test hook: replace the process-wide instance (tests run serially via
/// '#[serial_test::serial]').
#[cfg(test)]
pub fn reset_for_tests() {
    *GLOBAL.lock().unwrap() = None;
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    async fn wait_for(harness: &EvolutionHarness, group: &str) -> bool {
        for _ in 0..100 {
            if harness.genes().iter().any(|g| g.name == group && g.live) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        false
    }

    fn fitness_of(harness: &EvolutionHarness, group: &str) -> Option<i64> {
        harness
            .genes()
            .iter()
            .find(|g| g.name == group)
            .map(|g| g.fitness.score())
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn records_tool_group_fitness() {
        reset_for_tests();
        let dir = tempfile::tempdir().unwrap();
        let harness =
            EvolutionHarness::init(dir.path().to_path_buf(), EvolutionSettings::default());
        assert!(harness.enabled());

        harness.record_tool_result("FileRead", true);
        harness.record_tool_result("FileRead", true);
        harness.record_tool_result("Bash", false);
        assert!(wait_for(&harness, "filesystem").await);
        assert!(wait_for(&harness, "shell").await);

        assert_eq!(fitness_of(&harness, "filesystem"), Some(2));
        assert_eq!(fitness_of(&harness, "shell"), Some(-10));
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn tools_without_a_group_are_ignored() {
        reset_for_tests();
        let dir = tempfile::tempdir().unwrap();
        let harness =
            EvolutionHarness::init(dir.path().to_path_buf(), EvolutionSettings::default());
        // SkillTool / ToolSearch / MCP tools have no group: no gene.
        harness.record_tool_result("SkillTool", true);
        harness.record_tool_result("ToolSearch", true);
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(harness.genes().is_empty());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn disabled_harness_records_nothing() {
        reset_for_tests();
        let dir = tempfile::tempdir().unwrap();
        let harness = EvolutionHarness::init(
            dir.path().to_path_buf(),
            EvolutionSettings {
                enabled: false,
                ..Default::default()
            },
        );
        harness.record_tool_result("FileRead", true);
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(harness.genes().is_empty());
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn genome_persists_and_restores_with_fitness() {
        reset_for_tests();
        let dir = tempfile::tempdir().unwrap();

        // First run: observe usage, then shut down (persist on drop).
        {
            let harness =
                EvolutionHarness::init(dir.path().to_path_buf(), EvolutionSettings::default());
            harness.record_tool_result("FileRead", true);
            harness.record_tool_result("FileRead", true);
            harness.record_tool_result("GitStatus", false);
            assert!(wait_for(&harness, "filesystem").await);
            assert!(wait_for(&harness, "git").await);
            harness.persist();
            reset_for_tests(); // drop -> last-chance persist
        }

        // Second run: the genome restores with carried fitness.
        reset_for_tests();
        let harness =
            EvolutionHarness::init(dir.path().to_path_buf(), EvolutionSettings::default());
        // First touch triggers restore + dedupe against the restored gene.
        harness.record_tool_result("FileRead", true);
        assert!(wait_for(&harness, "filesystem").await);
        assert_eq!(fitness_of(&harness, "filesystem"), Some(3)); // 2 carried + 1 new
        assert_eq!(fitness_of(&harness, "git"), Some(-10));
        reset_for_tests();
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn unfit_groups_names_negative_score_genes() {
        reset_for_tests();
        let dir = tempfile::tempdir().unwrap();
        let harness =
            EvolutionHarness::init(dir.path().to_path_buf(), EvolutionSettings::default());
        harness.record_tool_result("WebFetch", true);
        harness.record_tool_result("TodoWrite", false);
        assert!(wait_for(&harness, "web").await);
        assert!(wait_for(&harness, "todo").await);

        let unfit = harness.unfit_groups();
        assert!(unfit.iter().any(|(name, _)| name == "todo"));
        assert!(!unfit.iter().any(|(name, _)| name == "web"));
    }
}
