//! Opt-in end-to-end team test against a REAL external agent CLI.
//!
//! Run with: `ZODE_EXTAGENT_IT=1 cargo test -p zode-core --test team_it -- --ignored`
//! Requires the `claude` CLI on PATH and a logged-in session; costs tokens.

use std::sync::Arc;
use std::time::Duration;

use zode_core::config::ExternalAgentsConfig;
use zode_core::external_agents::{discover, preapproval_fingerprint, GrantStore};
use zode_core::subagents::SubAgentRegistry;
use zode_core::team::{HireRequest, TeamDeps, TeamManager};

#[tokio::test]
#[ignore = "requires a real external CLI; run with ZODE_EXTAGENT_IT=1"]
async fn hire_send_resume_dismiss_real_claude() {
    if std::env::var("ZODE_EXTAGENT_IT").is_err() {
        eprintln!("ZODE_EXTAGENT_IT not set; skipping");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = ExternalAgentsConfig::default();
    cfg.agents.insert("claude-code".into(), Default::default());
    let reg = Arc::new(discover(&cfg, &[]));
    let def = reg.get("claude-code").expect("claude CLI on PATH").clone();

    // Pre-grant trust for the headless test (no interactive gate here).
    let grants = Arc::new(GrantStore::default());
    let mut fp = preapproval_fingerprint(&def, dir.path()).unwrap();
    fp.version_output = Some("it".into());
    grants.store_pending(&def.name, fp);
    grants.promote(&def.name, "it".into());

    let deps = TeamDeps {
        config: Arc::new(zode_core::config::ZodeConfig::default()),
        parent_provider: Default::default(),
        external_registry: reg,
        grants,
        observer: SubAgentRegistry::new().observer(),
        file_cache: Arc::new(agent::file_cache::FileStateCache::new(
            std::num::NonZeroUsize::new(8).unwrap(),
            1 << 20,
        )),
        runtime_spec: Default::default(),
        build_internal_tools: Arc::new(|_, _, _| Arc::new(agent::tool::ToolRegistry::new())),
        build_provider: Arc::new(|_| Err("n/a".into())),
        agent_def: Arc::new(|_| None),
        permissions: Arc::new(agent::permission::PermissionManager::new()),
        hooks: Arc::new(agent::hook::HookRunner::new()),
        timeout: Duration::from_secs(300),
    };

    let mgr = TeamManager::new(dir.path().to_path_buf());
    mgr.hire(
        &deps,
        HireRequest {
            agent: "claude-code".into(),
            name: "helper".into(),
            role: "assistant".into(),
            provider: None,
            model: None,
            tools: None,
        },
    )
    .await
    .unwrap();

    let out1 = mgr
        .send(
            &deps,
            "helper",
            "输出单词 pong",
            &[],
            agent::abort::AbortController::new(),
        )
        .await
        .unwrap();
    assert!(out1.reply.to_lowercase().contains("pong"), "{}", out1.reply);

    // Second send resumes the same session.
    let out2 = mgr
        .send(
            &deps,
            "helper",
            "再输出 ping",
            &[],
            agent::abort::AbortController::new(),
        )
        .await
        .unwrap();
    assert!(out2.reply.to_lowercase().contains("ping"), "{}", out2.reply);

    mgr.dismiss("helper").await.unwrap();
    assert!(mgr.roster().is_empty());
}
