//! Team tools — the model-facing surface. Two families:
//! * leader tools (registered in the main registry): team_hire / team_send /
//!   team_dismiss / team_list / team_board_* / team_claim / team_release.
//! * teammate collaboration tools ([`teammate_collab_tools`]): identity-bound
//!   board + claim tools handed to an internal teammate's own registry.
//!
//! Holder identity is bound at CONSTRUCTION, never read from tool input — a
//! shared instance could not otherwise distinguish callers (ToolUseContext
//! has no teammate identity).

use std::sync::Arc;

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde_json::{json, Value};

use super::board::{Board, BoardConflict};
use super::manager::{HireRequest, TeamDeps};
use super::{claims, TeamManager};
use crate::approval::{Approval, ApprovalGate, ApprovalScope};

/// Board read tool bound to a holder (for identity on claim/release).
#[derive(Debug)]
pub struct TeamBoardReadTool {
    board: Board,
}

impl TeamBoardReadTool {
    pub fn new(team_dir: std::path::PathBuf) -> Self {
        Self {
            board: Board::new(team_dir),
        }
    }
}
#[derive(Debug)]
pub struct TeamBoardUpdateTool {
    board: Board,
}
#[derive(Debug)]
pub struct TeamBoardAppendTool {
    board: Board,
}
#[derive(Debug)]
pub struct TeamClaimTool {
    board: Board,
    holder: String,
    cwd: std::path::PathBuf,
}
#[derive(Debug)]
pub struct TeamReleaseTool {
    board: Board,
    holder: String,
}

/// Roles whose teammates default to a READ-ONLY tool set (they may inspect
/// and use the board, but not mutate the workspace unless the caller widens
/// them — which `narrow_tools` refuses to do).
pub fn role_is_read_only(role: &str) -> bool {
    matches!(role, "reviewer" | "researcher")
}

/// Filter a teammate's tool registry: read-only roles keep only ReadOnly
/// tools (plus the always-collaboration board tools); an explicit `allowed`
/// list may only NARROW (names not already present are ignored — never
/// widens the safety ceiling). ToolSearch is rebuilt over the filtered set.
pub fn filter_teammate_tools(
    src: &agent::tool::ToolRegistry,
    role: &str,
    allowed: Option<&[String]>,
) -> Arc<agent::tool::ToolRegistry> {
    use agent::tool::SafetyClass;
    let read_only = role_is_read_only(role);
    // Collaboration tools are always kept regardless of role/allow.
    const ALWAYS: &[&str] = &[
        "team_board_read",
        "team_board_update",
        "team_board_append",
        "team_claim",
        "team_release",
        "ToolSearch",
    ];
    let mut out = agent::tool::ToolRegistry::new();
    for tool in src.list() {
        let name = tool.name();
        if name == "ToolSearch" {
            continue; // rebuilt below over the filtered set
        }
        let always = ALWAYS.contains(&name);
        if read_only && !always && !matches!(tool.safety_class(), SafetyClass::ReadOnly) {
            continue;
        }
        if let Some(allow) = allowed {
            if !always && !allow.iter().any(|a| a == name) {
                continue;
            }
        }
        out.register(tool);
    }
    let candidates = Arc::new(out.clone());
    out.register(Arc::new(agent_tools_code::ToolSearchTool::new(candidates)));
    Arc::new(out)
}

/// Build the identity-bound collaboration tools for a teammate `holder`.
pub fn teammate_collab_tools(mgr: &Arc<TeamManager>, holder: &str) -> Vec<Arc<dyn Tool>> {
    let board = Board::new(mgr.team_dir());
    let cwd = mgr.cwd.clone();
    vec![
        Arc::new(TeamBoardReadTool {
            board: board.clone(),
        }),
        Arc::new(TeamBoardUpdateTool {
            board: board.clone(),
        }),
        Arc::new(TeamBoardAppendTool {
            board: board.clone(),
        }),
        Arc::new(TeamClaimTool {
            board: board.clone(),
            holder: holder.to_string(),
            cwd,
        }),
        Arc::new(TeamReleaseTool {
            board,
            holder: holder.to_string(),
        }),
    ]
}

#[async_trait]
impl Tool for TeamBoardReadTool {
    fn name(&self) -> &str {
        "team_board_read"
    }
    fn description(&self) -> &str {
        "Read the shared team board (goal, sections, notes, claims)."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{}})
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::ReadOnly
    }
    async fn call(&self, _c: &ToolUseContext, _i: Value) -> Result<Value, AgentError> {
        let snap = self
            .board
            .read()
            .map_err(|e| AgentError::other(e.to_string()))?;
        serde_json::to_value(snap).map_err(|e| AgentError::other(e.to_string()))
    }
}

#[async_trait]
impl Tool for TeamBoardUpdateTool {
    fn name(&self) -> &str {
        "team_board_update"
    }
    fn description(&self) -> &str {
        "CAS-update a board section: {section, content, revision}. On a \
         revision mismatch the latest snapshot is returned so you can rebase."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","required":["section","content","revision"],
            "properties":{"section":{"type":"string"},"content":{"type":"string"},
            "revision":{"type":"integer"}}})
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }
    async fn call(&self, _c: &ToolUseContext, i: Value) -> Result<Value, AgentError> {
        let section = i.get("section").and_then(|v| v.as_str()).unwrap_or("");
        let content = i.get("content").and_then(|v| v.as_str()).unwrap_or("");
        let revision = i.get("revision").and_then(|v| v.as_u64()).unwrap_or(0);
        match self.board.update_section(section, content, revision) {
            Ok(snap) => Ok(json!({"ok": true, "revision": snap.revision})),
            Err(BoardConflict::Stale(latest)) => Ok(json!({
                "ok": false, "conflict": "stale_revision",
                "latest": serde_json::to_value(latest).unwrap_or(Value::Null)
            })),
            Err(BoardConflict::Team(e)) => Err(AgentError::other(e.to_string())),
        }
    }
}

#[async_trait]
impl Tool for TeamBoardAppendTool {
    fn name(&self) -> &str {
        "team_board_append"
    }
    fn description(&self) -> &str {
        "Append a durable note to the board: {note}."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","required":["note"],"properties":{"note":{"type":"string"}}})
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }
    async fn call(&self, _c: &ToolUseContext, i: Value) -> Result<Value, AgentError> {
        let note = i.get("note").and_then(|v| v.as_str()).unwrap_or("");
        self.board
            .append_note(note)
            .map_err(|e| AgentError::other(e.to_string()))?;
        Ok(json!({"ok": true}))
    }
}

const CLAIM_TTL: std::time::Duration = std::time::Duration::from_secs(1800);

#[async_trait]
impl Tool for TeamClaimTool {
    fn name(&self) -> &str {
        "team_claim"
    }
    fn description(&self) -> &str {
        "Reserve file paths for your exclusive edits: {paths:[...]}. Whole \
         batch is rejected if any path conflicts with another holder."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","required":["paths"],
            "properties":{"paths":{"type":"array","items":{"type":"string"}}}})
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }
    async fn call(&self, _c: &ToolUseContext, i: Value) -> Result<Value, AgentError> {
        let paths: Vec<String> = i
            .get("paths")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|p| p.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let now = now_ms();
        match claims::claim(&self.board, &self.holder, &paths, &self.cwd, CLAIM_TTL, now) {
            Ok(_) => Ok(json!({"ok": true, "claimed": paths})),
            Err(c) => Ok(json!({
                "ok": false, "conflict": serde_json::to_value(c.conflicts).unwrap_or(Value::Null)
            })),
        }
    }
}

#[async_trait]
impl Tool for TeamReleaseTool {
    fn name(&self) -> &str {
        "team_release"
    }
    fn description(&self) -> &str {
        "Release your claims: {paths:[...]} (omit to release all)."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{"paths":{"type":"array","items":{"type":"string"}}}})
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }
    async fn call(&self, _c: &ToolUseContext, i: Value) -> Result<Value, AgentError> {
        let paths: Option<Vec<String>> = i.get("paths").and_then(|v| v.as_array()).map(|a| {
            a.iter()
                .filter_map(|p| p.as_str().map(String::from))
                .collect()
        });
        claims::release(&self.board, &self.holder, paths.as_deref());
        Ok(json!({"ok": true}))
    }
}

pub(crate) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Leader tools — registered in the main registry. They carry the manager, a
// live-deps accessor, and the approval gate. `deps` is a closure so it always
// reflects the CURRENT engine build (a hot rebuild refreshes provider/tools).
// ---------------------------------------------------------------------------

type DepsFn = Arc<dyn Fn() -> TeamDeps + Send + Sync>;

#[derive(Clone)]
pub struct LeaderCtx {
    pub mgr: Arc<TeamManager>,
    pub deps: DepsFn,
    pub gate: Arc<dyn ApprovalGate>,
}

impl std::fmt::Debug for LeaderCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LeaderCtx").finish()
    }
}

/// Build the leader-facing team tool set for the main registry.
pub fn leader_tools(ctx: LeaderCtx) -> Vec<Arc<dyn Tool>> {
    let board = Board::new(ctx.mgr.team_dir());
    vec![
        Arc::new(TeamHireTool { ctx: ctx.clone() }),
        Arc::new(TeamSendTool { ctx: ctx.clone() }),
        Arc::new(TeamDismissTool { ctx: ctx.clone() }),
        Arc::new(TeamListTool { ctx: ctx.clone() }),
        Arc::new(TeamBoardReadTool {
            board: board.clone(),
        }),
        Arc::new(TeamBoardUpdateTool {
            board: board.clone(),
        }),
        Arc::new(TeamBoardAppendTool { board }),
    ]
}

/// Names of every leader team tool (for plugin group + plan-mode filtering).
pub const LEADER_TOOL_NAMES: &[&str] = &[
    "team_hire",
    "team_send",
    "team_dismiss",
    "team_list",
    "team_board_read",
    "team_board_update",
    "team_board_append",
];

#[derive(Debug)]
pub struct TeamHireTool {
    ctx: LeaderCtx,
}
#[derive(Debug)]
pub struct TeamSendTool {
    ctx: LeaderCtx,
}
#[derive(Debug)]
pub struct TeamDismissTool {
    ctx: LeaderCtx,
}
#[derive(Debug)]
pub struct TeamListTool {
    ctx: LeaderCtx,
}

impl TeamListTool {
    pub fn new(ctx: LeaderCtx) -> Self {
        Self { ctx }
    }
}

#[async_trait]
impl Tool for TeamHireTool {
    fn name(&self) -> &str {
        "team_hire"
    }
    fn description(&self) -> &str {
        "Hire a persistent teammate: {agent, name, role, provider?, model?, \
         tools?}. `agent` is an external CLI profile or a built-in/AgentDef \
         type. External hires require a one-time trust approval."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","required":["agent","name","role"],"properties":{
            "agent":{"type":"string"},"name":{"type":"string"},"role":{"type":"string"},
            "provider":{"type":"string"},"model":{"type":"string"},
            "tools":{"type":"array","items":{"type":"string"}}}})
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }
    async fn call(&self, _c: &ToolUseContext, i: Value) -> Result<Value, AgentError> {
        let req = HireRequest {
            agent: str_field(&i, "agent"),
            name: str_field(&i, "name"),
            role: str_field(&i, "role"),
            provider: opt_field(&i, "provider"),
            model: opt_field(&i, "model"),
            tools: i.get("tools").and_then(|v| v.as_array()).map(|a| {
                a.iter()
                    .filter_map(|t| t.as_str().map(String::from))
                    .collect()
            }),
        };
        let deps = (self.ctx.deps)();
        // External hire → one-time trust approval (session scope).
        if let Some(def) = deps.external_registry.get(&req.agent) {
            let view = json!({
                "_kind": "external-agent",
                "_agent": def.name,
                "_command": std::iter::once(def.command.display().to_string())
                    .chain(def.args.iter().cloned()).collect::<Vec<_>>().join(" "),
                "_cwd": self.ctx.mgr.cwd.display().to_string(),
                "_role": req.role,
                "_sandbox": def.capability.effective_sandbox.to_string(),
                "_version": "unverified",
            });
            if !self.ctx.gate.interactive() && !def.trusted {
                return Err(AgentError::other(format!(
                    "hiring external agent '{}' needs an interactive trust approval \
                     (or set externalAgents.agents.{}.trusted=true)",
                    def.name, def.name
                )));
            }
            if self.ctx.gate.interactive() {
                match self
                    .ctx
                    .gate
                    .approve_scoped("team_hire", &view, ApprovalScope::TeamMemberSession)
                    .await
                {
                    Approval::Deny => {
                        return Err(AgentError::other(format!(
                            "hiring '{}' denied by user",
                            def.name
                        )))
                    }
                    _ => {
                        // Store a pending grant so the first send can promote it.
                        if let Ok(fp) =
                            crate::external_agents::preapproval_fingerprint(def, &self.ctx.mgr.cwd)
                        {
                            deps.grants.store_pending(&def.name, fp);
                        }
                    }
                }
            }
        }
        self.ctx
            .mgr
            .hire(&deps, req)
            .await
            .map(|_| json!({"ok": true}))
            .map_err(|e| AgentError::other(e.to_string()))
    }
}

#[async_trait]
impl Tool for TeamSendTool {
    fn name(&self) -> &str {
        "team_send"
    }
    fn description(&self) -> &str {
        "Send a task to a teammate and wait for the reply: {to, message, \
         claims?}. `claims` reserves file paths for that teammate first \
         (all-or-nothing). Returns the reply plus any @ask relays to forward."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","required":["to","message"],"properties":{
            "to":{"type":"string"},"message":{"type":"string"},
            "claims":{"type":"array","items":{"type":"string"}}}})
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }
    async fn call(&self, ctx: &ToolUseContext, i: Value) -> Result<Value, AgentError> {
        let to = str_field(&i, "to");
        let message = str_field(&i, "message");
        let claim_paths: Vec<String> = i
            .get("claims")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|p| p.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let deps = (self.ctx.deps)();
        let out = self
            .ctx
            .mgr
            .send(&deps, &to, &message, &claim_paths, ctx.abort.child())
            .await
            .map_err(|e| AgentError::other(e.to_string()))?;
        Ok(json!({
            "reply": out.reply,
            "asks": out.asks.iter().map(|a| json!({"to": a.to, "question": a.question}))
                .collect::<Vec<_>>(),
            "changed_files": out.changed_files,
            "warnings": out.warnings,
        }))
    }
}

#[async_trait]
impl Tool for TeamDismissTool {
    fn name(&self) -> &str {
        "team_dismiss"
    }
    fn description(&self) -> &str {
        "Dismiss a teammate and release its claims: {name}."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","required":["name"],"properties":{"name":{"type":"string"}}})
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::Mutating
    }
    async fn call(&self, _c: &ToolUseContext, i: Value) -> Result<Value, AgentError> {
        self.ctx
            .mgr
            .dismiss(&str_field(&i, "name"))
            .await
            .map(|_| json!({"ok": true}))
            .map_err(|e| AgentError::other(e.to_string()))
    }
}

#[async_trait]
impl Tool for TeamListTool {
    fn name(&self) -> &str {
        "team_list"
    }
    fn description(&self) -> &str {
        "List current teammates and their status."
    }
    fn input_schema(&self) -> Value {
        json!({"type":"object","properties":{}})
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::ReadOnly
    }
    async fn call(&self, _c: &ToolUseContext, _i: Value) -> Result<Value, AgentError> {
        let roster = self.ctx.mgr.roster();
        Ok(json!({"teammates": roster.iter().map(|t| json!({
            "name": t.name, "role": t.role, "model": t.model_label,
            "status": t.status_line, "usage_in": t.usage_in, "usage_out": t.usage_out,
        })).collect::<Vec<_>>()}))
    }
}

fn str_field(v: &Value, k: &str) -> String {
    v.get(k).and_then(|x| x.as_str()).unwrap_or("").to_string()
}
fn opt_field(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(|x| x.as_str()).map(String::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn claim_tool_holder_cannot_be_forged() {
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(cwd.join("src")).unwrap();
        let board = Board::new(cwd.join(".zode/team"));
        let tool = TeamClaimTool {
            board: board.clone(),
            holder: "alice".to_string(),
            cwd: cwd.clone(),
        };
        let ctx = ToolUseContext::new(&cwd);
        // Model tries to forge holder via input — ignored; entry is alice's.
        let out = tool
            .call(&ctx, json!({"paths":["src/a.rs"],"holder":"bob"}))
            .await
            .unwrap();
        assert_eq!(out["ok"], true);
        let snap = board.read().unwrap();
        assert_eq!(snap.claims[0].holder, "alice");
    }

    #[test]
    fn read_only_role_filters_out_mutating_tools() {
        use agent::tool::{SafetyClass, Tool, ToolUseContext};
        use async_trait::async_trait;

        #[derive(Debug)]
        struct Fake(&'static str, SafetyClass);
        #[async_trait]
        impl Tool for Fake {
            fn name(&self) -> &str {
                self.0
            }
            fn description(&self) -> &str {
                "x"
            }
            fn input_schema(&self) -> Value {
                json!({"type":"object"})
            }
            fn safety_class(&self) -> SafetyClass {
                self.1
            }
            async fn call(&self, _c: &ToolUseContext, _i: Value) -> Result<Value, AgentError> {
                Ok(json!({}))
            }
        }

        let mut src = agent::tool::ToolRegistry::new();
        src.register(Arc::new(Fake("FileRead", SafetyClass::ReadOnly)));
        src.register(Arc::new(Fake("FileWrite", SafetyClass::Mutating)));
        src.register(Arc::new(Fake("team_board_read", SafetyClass::ReadOnly)));

        // reviewer → mutating tool dropped, read-only kept, board always kept.
        let out = filter_teammate_tools(&src, "reviewer", None);
        assert!(out.get("FileRead").is_some());
        assert!(
            out.get("FileWrite").is_none(),
            "mutating dropped for reviewer"
        );
        assert!(out.get("team_board_read").is_some());

        // general role keeps everything; an allow-list only narrows.
        let out = filter_teammate_tools(&src, "general", Some(&["FileRead".to_string()]));
        assert!(out.get("FileRead").is_some());
        assert!(out.get("FileWrite").is_none(), "narrowed away");
    }

    #[tokio::test]
    async fn board_update_cas_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let board = Board::new(dir.path().join(".zode/team"));
        let tool = TeamBoardUpdateTool {
            board: board.clone(),
        };
        let ctx = ToolUseContext::new(dir.path());
        let out = tool
            .call(&ctx, json!({"section":"plan","content":"v1","revision":0}))
            .await
            .unwrap();
        assert_eq!(out["ok"], true);
        // stale revision → conflict with latest
        let out = tool
            .call(&ctx, json!({"section":"plan","content":"v2","revision":0}))
            .await
            .unwrap();
        assert_eq!(out["ok"], false);
        assert_eq!(out["conflict"], "stale_revision");
    }
}
