use std::path::{Path, PathBuf};

use crate::config::NoemaSettings;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZodeMemoryScope {
    User,
    Project,
}

#[derive(Debug, Clone)]
pub struct ZodeNoema {
    root: Option<PathBuf>,
    user: Option<String>,
    auto_remember: bool,
}

impl ZodeNoema {
    pub fn disabled() -> Self {
        Self {
            root: None,
            user: None,
            auto_remember: false,
        }
    }

    pub fn from_root(root: impl AsRef<Path>) -> Self {
        Self::from_root_with_user(root, None::<String>)
    }

    pub fn from_root_with_user(root: impl AsRef<Path>, user: impl Into<Option<String>>) -> Self {
        Self {
            root: Some(root.as_ref().to_path_buf()),
            user: user.into().filter(|value| !value.trim().is_empty()),
            auto_remember: true,
        }
    }

    pub fn from_settings(settings: &NoemaSettings) -> Self {
        if !settings.enabled() {
            return Self::disabled();
        }

        #[cfg(feature = "noema")]
        {
            let loaded = noema_core::config::NoemaConfig::load_or_default().ok();
            let root = settings
                .root
                .as_ref()
                .filter(|value| !value.trim().is_empty())
                .map(PathBuf::from)
                .or_else(|| loaded.as_ref().map(|cfg| cfg.storage.local_root.clone()))
                .unwrap_or_else(noema_core::config::default_root);
            let user = settings
                .user
                .as_ref()
                .filter(|value| !value.trim().is_empty())
                .cloned()
                .or_else(|| loaded.map(|cfg| cfg.tenant.default_user_id))
                .or_else(default_user);
            let adapter = Self {
                root: Some(root),
                user,
                auto_remember: settings.auto_remember(),
            };
            // Apply the effective write policy at startup so subsequent
            // `NoemaEngine::new(root)` loads observe it. The policy is
            // deterministic (review when extraction is off, autoSafe when on),
            // and `apply_write_policy` only writes when it actually differs —
            // so this is reversible and doesn't churn config.toml each launch.
            // Best-effort: a failure here must not disable memory.
            adapter.apply_write_policy(&settings.effective_write_policy());
            adapter
        }

        #[cfg(not(feature = "noema"))]
        {
            Self::disabled()
        }
    }

    /// Persist a write policy to the noema root so it governs future submits.
    /// `policy` is a lowercased keyword (`manual`/`review`/`autosafe`/`auto`);
    /// unknown values are ignored. Best-effort — errors are logged, not fatal.
    #[cfg(feature = "noema")]
    fn apply_write_policy(&self, policy: &str) {
        use noema_core::api::{NoemaEngine, PolicySetRequest};
        use noema_core::config::WritePolicy;

        let Some(root) = &self.root else { return };
        let write = match policy {
            "manual" => WritePolicy::Manual,
            "review" => WritePolicy::Review,
            "autosafe" => WritePolicy::AutoSafe,
            "auto" => WritePolicy::Auto,
            other => {
                tracing::debug!(policy = other, "unknown noema write policy; ignoring");
                return;
            }
        };
        let principal = self.principal();
        let result = NoemaEngine::new(root).and_then(|engine| {
            // Only persist when the policy actually differs, so we don't rewrite
            // config.toml on every launch (and don't stomp an externally-managed
            // tenant whose policy already matches).
            if engine.policy_get(&principal)?.write == write {
                return Ok(());
            }
            engine
                .policy_set(PolicySetRequest {
                    principal,
                    write: Some(write),
                })
                .map(|_| ())
        });
        if let Err(err) = result {
            tracing::debug!(error = %err, "failed to apply noema write policy");
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.root.is_some()
    }

    pub fn root(&self) -> Option<&Path> {
        self.root.as_deref()
    }

    pub fn user(&self) -> Option<&str> {
        self.user.as_deref()
    }

    pub fn auto_remember_enabled(&self) -> bool {
        self.auto_remember
    }

    pub fn status_line(&self) -> String {
        let Some(root) = &self.root else {
            return "memory: disabled".to_string();
        };
        let state = if root.exists() {
            "ready"
        } else {
            "not initialized"
        };
        format!(
            "memory: on ({state})\nroot: {}\nuser: {}",
            root.display(),
            self.user_id()
        )
    }

    pub fn remember(
        &self,
        text: &str,
        scope: ZodeMemoryScope,
        cwd: Option<&Path>,
    ) -> Result<String, String> {
        let text = text.trim();
        if text.is_empty() {
            return Err("usage: /memory remember [project|user] <text>".to_string());
        }
        let Some(root) = &self.root else {
            return Err("memory is disabled".to_string());
        };

        #[cfg(feature = "noema")]
        {
            use noema_core::api::{
                NoemaEngine, ReviewAction, ReviewDecisionRequest, ReviewOutcome, SubmitOutcome,
            };

            let engine = NoemaEngine::new(root).map_err(|err| err.to_string())?;
            // The old direct-write remember_text was removed to close the governance
            // bypass, so we go through submit_candidate. An explicit user "remember"
            // should persist immediately (the review queue is for auto-extracted
            // candidates, not explicit requests), so if it queues we accept it now.
            let outcome = engine
                .submit_candidate(self.build_remember_request(text, scope, cwd))
                .map_err(|err| err.to_string())?;
            let memory_id = match outcome {
                SubmitOutcome::AutoAccepted { memory_id } => memory_id,
                SubmitOutcome::Queued { candidate_id } => match engine
                    .review_decide(ReviewDecisionRequest {
                        principal: self.principal(),
                        candidate_id,
                        action: ReviewAction::Accept,
                    })
                    .map_err(|err| err.to_string())?
                {
                    ReviewOutcome::Accepted { memory_id } => memory_id,
                    other => return Err(format!("unexpected review outcome: {other:?}")),
                },
                SubmitOutcome::RejectedSecret => {
                    return Err("rejected: secret-class content cannot be stored".to_string())
                }
                SubmitOutcome::DuplicatePending { candidate_id } => {
                    tracing::debug!(%candidate_id, "memory already pending review");
                    candidate_id
                }
            };
            Ok(format!("remembered {memory_id}"))
        }

        #[cfg(not(feature = "noema"))]
        {
            Err("noema support is not compiled in".to_string())
        }
    }

    pub fn search(&self, query: &str, cwd: Option<&Path>) -> Result<String, String> {
        let query = query.trim();
        if query.is_empty() {
            return Err("usage: /memory search <query>".to_string());
        }
        Ok(self
            .recall_for_turn(query, cwd)?
            .unwrap_or_else(|| "(no matching memories)".to_string()))
    }

    pub fn auto_remember_from_turn(
        &self,
        user_input: &str,
        cwd: Option<&Path>,
    ) -> Result<Option<String>, String> {
        if !self.auto_remember {
            return Ok(None);
        }
        let Some(candidate) = extract_auto_memory(user_input) else {
            return Ok(None);
        };
        self.remember(candidate, ZodeMemoryScope::User, cwd)
            .map(Some)
    }

    /// Submit LLM-extracted candidates through noema's normal governance
    /// (route_candidate decides auto-accept vs review). Unlike the explicit
    /// [`remember`](Self::remember) path, this does NOT force-accept queued
    /// items — low-confidence/duplicate candidates stay in the review inbox.
    /// Returns one outcome string per candidate; never errors as a whole (a
    /// per-item failure is reported in that item's slot). Best-effort: the
    /// caller runs this off the turn's critical path.
    #[cfg(feature = "noema")]
    pub fn submit_extracted(
        &self,
        items: &[crate::noema_extract::ExtractedCandidate],
        cwd: Option<&Path>,
    ) -> Vec<Result<String, String>> {
        use noema_core::api::{NoemaEngine, SubmitOutcome};

        let Some(root) = &self.root else {
            return vec![Err("memory is disabled".to_string()); items.len()];
        };
        let engine = match NoemaEngine::new(root) {
            Ok(engine) => engine,
            Err(err) => return vec![Err(err.to_string()); items.len()],
        };
        items
            .iter()
            .map(|item| {
                let req = self.candidate_request(item, cwd);
                match engine.submit_candidate(req) {
                    Ok(SubmitOutcome::AutoAccepted { memory_id }) => {
                        Ok(format!("stored {memory_id}"))
                    }
                    Ok(SubmitOutcome::Queued { candidate_id }) => {
                        Ok(format!("queued {candidate_id}"))
                    }
                    Ok(SubmitOutcome::RejectedSecret) => {
                        Err("rejected: secret-class content".to_string())
                    }
                    Ok(SubmitOutcome::DuplicatePending { candidate_id }) => {
                        tracing::debug!(%candidate_id, "extracted memory already pending review");
                        Ok(format!("queued {candidate_id}"))
                    }
                    Err(err) => Err(err.to_string()),
                }
            })
            .collect()
    }

    /// Map an extracted candidate onto a noema `RememberRequest`, honoring the
    /// model's own kind/scope/sensitivity/importance/confidence (not the
    /// hardcoded defaults the explicit path uses).
    #[cfg(feature = "noema")]
    fn candidate_request(
        &self,
        item: &crate::noema_extract::ExtractedCandidate,
        cwd: Option<&Path>,
    ) -> noema_core::api::RememberRequest {
        use crate::noema_extract::{MemoryKindHint, SensitivityHint};
        use noema_core::api::RememberRequest;
        use noema_core::memory::{MemoryKind, Scope};
        use noema_core::sensitivity::SensitivityLevel;

        let scope = match item.scope {
            ZodeMemoryScope::User => Scope::User,
            ZodeMemoryScope::Project => Scope::Project,
        };
        let project_path = matches!(item.scope, ZodeMemoryScope::Project)
            .then(|| cwd.map(Path::to_path_buf))
            .flatten();
        let kind = match item.kind {
            MemoryKindHint::Preference => MemoryKind::Preference,
            MemoryKindHint::Decision => MemoryKind::Decision,
            MemoryKindHint::Constraint => MemoryKind::Constraint,
            MemoryKindHint::Fact => MemoryKind::Fact,
            MemoryKindHint::Reference => MemoryKind::Reference,
            MemoryKindHint::Workflow => MemoryKind::Workflow,
            MemoryKindHint::Warning => MemoryKind::Warning,
            MemoryKindHint::ErrorFix => MemoryKind::ErrorFix,
        };
        let sensitivity = match item.sensitivity {
            SensitivityHint::Public => SensitivityLevel::Public,
            SensitivityHint::Internal => SensitivityLevel::Internal,
        };
        RememberRequest {
            principal: self.principal(),
            text: item.body.clone(),
            scope,
            project_path,
            context_cwd: cwd.map(Path::to_path_buf),
            code: item
                .code
                .as_ref()
                .map(|code| noema_core::memory::CodeAnchor {
                    repo: None,
                    paths: code
                        .paths
                        .iter()
                        .filter_map(|path| {
                            cwd.and_then(|cwd| {
                                noema_core::project::host_relativize(Path::new(path), cwd)
                            })
                        })
                        .collect(),
                    symbols: code.symbols.clone(),
                    head_commit: code.head_commit.clone(),
                    ref_commits: code.ref_commits.clone(),
                    lang: code.lang.clone(),
                    error_sig: code.error_sig.clone(),
                    commands: code.commands.clone(),
                }),
            kind,
            sensitivity,
            tags: item.tags.clone(),
            entities: item.entities.clone(),
            confidence: item.confidence,
            importance: item.importance,
        }
    }

    #[cfg(feature = "noema")]
    pub(crate) fn build_remember_request(
        &self,
        text: &str,
        scope: ZodeMemoryScope,
        cwd: Option<&Path>,
    ) -> noema_core::api::RememberRequest {
        use noema_core::api::RememberRequest;
        use noema_core::memory::{MemoryKind, Scope};
        use noema_core::sensitivity::SensitivityLevel;

        RememberRequest {
            principal: self.principal(),
            text: text.to_string(),
            scope: match scope {
                ZodeMemoryScope::User => Scope::User,
                ZodeMemoryScope::Project => Scope::Project,
            },
            project_path: matches!(scope, ZodeMemoryScope::Project)
                .then(|| cwd.map(Path::to_path_buf))
                .flatten(),
            context_cwd: cwd.map(Path::to_path_buf),
            code: None,
            kind: MemoryKind::Preference,
            sensitivity: SensitivityLevel::Internal,
            tags: Vec::new(),
            entities: Vec::new(),
            confidence: 1.0,
            importance: 0.5,
        }
    }

    pub fn recall_for_turn(
        &self,
        query: &str,
        cwd: Option<&Path>,
    ) -> Result<Option<String>, String> {
        let query = query.trim();
        if query.is_empty() {
            return Ok(None);
        }
        let Some(root) = &self.root else {
            return Ok(None);
        };
        if !root.exists() {
            return Ok(None);
        }

        #[cfg(feature = "noema")]
        {
            use noema_core::api::{NoemaEngine, RecallRequest};

            let engine = NoemaEngine::new(root).map_err(|err| err.to_string())?;
            let pack = engine
                .recall(RecallRequest {
                    principal: self.principal(),
                    query: query.to_string(),
                    cwd: cwd.map(Path::to_path_buf),
                    budget_tokens: 1200,
                })
                .map_err(|err| err.to_string())?;
            if pack.memories.is_empty() && pack.subconscious_hints.is_empty() {
                return Ok(None);
            }
            Ok(Some(pack.to_markdown()))
        }

        #[cfg(not(feature = "noema"))]
        {
            Ok(None)
        }
    }

    pub fn prepend_for_turn(&self, user_input: &str, cwd: Option<&Path>) -> String {
        match self.recall_for_turn(user_input, cwd) {
            Ok(Some(memory)) => format!("{memory}\n\n{user_input}"),
            Ok(None) => user_input.to_string(),
            Err(err) => {
                tracing::debug!(error = %err, "memory unavailable");
                user_input.to_string()
            }
        }
    }

    pub fn handle_command(&self, args: &str, cwd: Option<&Path>) -> String {
        let trimmed = args.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("status") {
            return self.status_line();
        }
        if trimmed.eq_ignore_ascii_case("help") {
            return memory_usage().to_string();
        }
        let Some((verb, rest)) = split_word(trimmed) else {
            return memory_usage().to_string();
        };
        match verb {
            "remember" => {
                let (scope, text) = parse_remember_scope(rest);
                self.remember(text, scope, cwd)
                    .unwrap_or_else(|err| format!("memory: {err}"))
            }
            "search" | "recall" => self
                .search(rest, cwd)
                .unwrap_or_else(|err| format!("memory: {err}")),
            "root" => self
                .root()
                .map(|root| root.display().to_string())
                .unwrap_or_else(|| "memory: disabled".to_string()),
            _ => memory_usage().to_string(),
        }
    }

    fn user_id(&self) -> String {
        self.user
            .clone()
            .or_else(default_user)
            .unwrap_or_else(|| "user".to_string())
    }

    #[cfg(feature = "noema")]
    fn principal(&self) -> noema_core::sensitivity::Principal {
        noema_core::sensitivity::Principal::personal(self.user_id(), "zode")
    }
}

fn parse_remember_scope(input: &str) -> (ZodeMemoryScope, &str) {
    let trimmed = input.trim();
    let Some((first, rest)) = split_word(trimmed) else {
        return (ZodeMemoryScope::User, trimmed);
    };
    match first {
        "project" | "--project" => (ZodeMemoryScope::Project, rest.trim()),
        "user" | "--user" => (ZodeMemoryScope::User, rest.trim()),
        _ => (ZodeMemoryScope::User, trimmed),
    }
}

fn extract_auto_memory(input: &str) -> Option<&str> {
    let trimmed = strip_invocation_prefix(input.trim());
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("do not remember")
        || lower.starts_with("don't remember")
        || trimmed.starts_with("不要记住")
        || trimmed.starts_with("别记住")
    {
        return None;
    }

    for marker in [
        "please remember that",
        "please remember",
        "remember that",
        "remember:",
        "remember",
    ] {
        if lower.starts_with(marker) {
            let candidate = cleanup_auto_memory_candidate(&trimmed[marker.len()..]);
            return valid_auto_memory_candidate(candidate);
        }
    }

    for marker in [
        "请记住",
        "帮我记住",
        "麻烦记住",
        "记住：",
        "记住:",
        "记住",
        "记一下",
    ] {
        if let Some(rest) = trimmed.strip_prefix(marker) {
            let candidate = cleanup_auto_memory_candidate(rest);
            return valid_auto_memory_candidate(candidate);
        }
    }

    let candidate = cleanup_auto_memory_candidate(trimmed);
    if looks_like_stable_memory_statement(candidate) {
        return valid_auto_memory_candidate(candidate);
    }

    None
}

fn looks_like_stable_memory_statement(input: &str) -> bool {
    let trimmed = input.trim();
    if trimmed.is_empty() || is_likely_question(trimmed) {
        return false;
    }

    if [
        "喜欢",
        "不喜欢",
        "爱吃",
        "爱喝",
        "偏好",
        "讨厌",
        "不能吃",
        "过敏",
        "常用",
        "习惯",
        "默认用",
        "固定用",
    ]
    .iter()
    .any(|marker| trimmed.contains(marker))
    {
        return true;
    }

    let lower = trimmed.to_ascii_lowercase();
    let padded = format!(" {lower} ");
    lower.starts_with("i prefer ")
        || lower.starts_with("i like ")
        || lower.starts_with("i love ")
        || lower.starts_with("i dislike ")
        || lower.starts_with("my preference ")
        || lower.starts_with("my favorite ")
        || lower.starts_with("we prefer ")
        || lower.starts_with("we use ")
        || padded.contains(" prefers ")
        || padded.contains(" likes ")
        || padded.contains(" dislikes ")
        || padded.contains(" usually uses ")
}

fn is_likely_question(input: &str) -> bool {
    let trimmed = input.trim();
    trimmed.ends_with('?')
        || trimmed.ends_with('？')
        || [
            "什么",
            "吗",
            "嘛",
            "么",
            "怎么",
            "如何",
            "为什么",
            "谁",
            "哪里",
            "哪",
            "多少",
        ]
        .iter()
        .any(|marker| trimmed.contains(marker))
}

fn strip_invocation_prefix(input: &str) -> &str {
    let trimmed = input.trim_start();
    for prefix in ["zode", "Zode", "zode，", "zode,", "Zode，", "Zode,"] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            return rest
                .trim_start_matches([' ', '\t', ',', '，', ':', '：'])
                .trim_start();
        }
    }
    trimmed
}

fn cleanup_auto_memory_candidate(input: &str) -> &str {
    input
        .trim()
        .trim_start_matches([' ', '\t', ':', '：', ',', '，', '-', '—'])
        .trim()
        .trim_matches(['"', '\'', '“', '”', '‘', '’'])
        .trim()
}

fn valid_auto_memory_candidate(input: &str) -> Option<&str> {
    let chars = input.chars().count();
    if (4..=2000).contains(&chars) {
        Some(input)
    } else {
        None
    }
}

fn split_word(input: &str) -> Option<(&str, &str)> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.split_once(char::is_whitespace) {
        Some((first, rest)) => Some((first, rest.trim())),
        None => Some((trimmed, "")),
    }
}

fn memory_usage() -> &'static str {
    "usage: /memory [status|remember [project|user] <text>|search <query>|root]"
}

fn default_user() -> Option<String> {
    std::env::var("USER")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_noema_returns_empty_pack() {
        let adapter = ZodeNoema::disabled();
        let pack = adapter.recall_for_turn("hello", None).unwrap();
        assert!(pack.is_none());
    }

    #[test]
    fn unavailable_noema_is_non_fatal() {
        let adapter = ZodeNoema::from_root("/path/that/does/not/exist");
        let pack = adapter.recall_for_turn("rust memory", None).unwrap();
        assert!(pack.is_none());
    }

    #[test]
    fn memory_command_parses_scope_prefixes() {
        assert_eq!(
            parse_remember_scope("project Prefer Rust.").0,
            ZodeMemoryScope::Project
        );
        assert_eq!(
            parse_remember_scope("--user Prefer Rust.").0,
            ZodeMemoryScope::User
        );
        assert_eq!(
            parse_remember_scope("Prefer Rust.").0,
            ZodeMemoryScope::User
        );
    }

    #[test]
    fn auto_memory_extracts_explicit_and_stable_memory_requests() {
        assert_eq!(
            extract_auto_memory("请记住我喜欢 Rust 工具"),
            Some("我喜欢 Rust 工具")
        );
        assert_eq!(
            extract_auto_memory("zode, remember that I prefer Rust."),
            Some("I prefer Rust.")
        );
        assert_eq!(
            extract_auto_memory("王小明爱吃酸的"),
            Some("王小明爱吃酸的")
        );
        assert_eq!(extract_auto_memory("王小明爱吃什么"), None);
        assert_eq!(
            extract_auto_memory("刚才是测试让它记住东西，换 session 没有记忆"),
            None
        );
        assert_eq!(extract_auto_memory("不要记住这个临时内容"), None);
    }

    #[cfg(feature = "noema")]
    fn candidate(
        body: &str,
        confidence: f32,
        importance: f32,
    ) -> crate::noema_extract::ExtractedCandidate {
        use crate::noema_extract::{ExtractedCandidate, MemoryKindHint, SensitivityHint};
        ExtractedCandidate {
            body: body.to_string(),
            entities: Vec::new(),
            tags: Vec::new(),
            kind: MemoryKindHint::Preference,
            scope: ZodeMemoryScope::User,
            sensitivity: SensitivityHint::Internal,
            importance,
            confidence,
            code: None,
        }
    }

    #[cfg(feature = "noema")]
    #[test]
    fn noema_mapping_produces_error_fix_request() {
        use crate::noema_extract::{ExtractedCode, MemoryKindHint};
        use noema_core::memory::MemoryKind;

        let adapter = ZodeNoema::from_root_with_user("/tmp/noema", Some("kay".to_string()));
        let mut item = candidate("Convert before returning", 0.9, 0.7);
        item.kind = MemoryKindHint::ErrorFix;
        item.code = Some(ExtractedCode {
            error_sig: Some("error[E0308]: mismatched types".into()),
            ..Default::default()
        });
        let cwd = Path::new("/tmp/project");
        let request = adapter.candidate_request(&item, Some(cwd));
        assert_eq!(request.kind, MemoryKind::ErrorFix);
        assert_eq!(request.context_cwd.as_deref(), Some(cwd));
    }

    #[cfg(feature = "noema")]
    #[test]
    fn cwd_relative_paths_convert_in_submit_extracted() {
        use crate::noema_extract::ExtractedCode;

        // Keep the fixture off macOS's /var -> /private/var symlink so the
        // lexical (deliberately non-canonicalizing) path contract is tested.
        let repo = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        std::fs::create_dir_all(repo.path().join("docs")).unwrap();
        std::fs::create_dir_all(repo.path().join("src")).unwrap();
        std::fs::write(repo.path().join("src/a.rs"), "pub fn a() {}\n").unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(repo.path())
            .status()
            .unwrap()
            .success());
        let cwd = repo.path().join("docs");
        let adapter = ZodeNoema::from_root("/tmp/noema");
        let mut item = candidate("Use the source fix", 0.9, 0.7);
        item.code = Some(ExtractedCode {
            paths: vec!["../src/a.rs".into()],
            ..Default::default()
        });
        let request = adapter.candidate_request(&item, Some(&cwd));
        assert_eq!(request.code.unwrap().paths, vec!["src/a.rs"]);
    }

    #[cfg(feature = "noema")]
    #[test]
    fn explicit_remember_passes_capture_cwd() {
        let adapter = ZodeNoema::from_root_with_user("/tmp/noema", Some("kay".to_string()));
        let cwd = Path::new("/tmp/capture-cwd");
        let request =
            adapter.build_remember_request("Prefer Rust", ZodeMemoryScope::User, Some(cwd));
        assert_eq!(request.context_cwd.as_deref(), Some(cwd));
        assert!(request.code.is_none());
    }

    #[cfg(feature = "noema")]
    #[test]
    #[serial_test::serial]
    fn submit_extracted_auto_accepts_high_confidence_under_autosafe() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("NOEMA_ROOT", dir.path());
        let adapter = ZodeNoema::from_settings(&NoemaSettings {
            auto_extract: Some(true), // → autoSafe policy applied
            user: Some("kay".into()),
            ..Default::default()
        });
        std::env::remove_var("NOEMA_ROOT");

        // High confidence + importance + novel → auto-accepted.
        let items = vec![candidate("User prefers ripgrep for search", 0.95, 0.7)];
        let outcomes = adapter.submit_extracted(&items, None);
        assert_eq!(outcomes.len(), 1);
        assert!(
            outcomes[0].as_ref().unwrap().starts_with("stored "),
            "expected stored, got {:?}",
            outcomes[0]
        );

        // And it is recallable.
        let recalled = adapter.recall_for_turn("ripgrep search", None).unwrap();
        assert!(recalled.is_some());
    }

    #[cfg(feature = "noema")]
    #[test]
    #[serial_test::serial]
    fn submit_extracted_queues_low_confidence_without_force_accept() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("NOEMA_ROOT", dir.path());
        let adapter = ZodeNoema::from_settings(&NoemaSettings {
            auto_extract: Some(true),
            user: Some("kay".into()),
            ..Default::default()
        });
        std::env::remove_var("NOEMA_ROOT");

        // Below the 0.80 confidence gate → queued, NOT stored (no force-accept).
        let items = vec![candidate("User might like vim", 0.4, 0.5)];
        let outcomes = adapter.submit_extracted(&items, None);
        assert!(
            outcomes[0].as_ref().unwrap().starts_with("queued "),
            "expected queued, got {:?}",
            outcomes[0]
        );

        // It must NOT be recallable yet (only the inbox holds it).
        let recalled = adapter.recall_for_turn("vim", None).unwrap();
        assert!(recalled.is_none(), "queued item must not be recallable");

        // Submitting the same low-confidence item again yields
        // DuplicatePending, which is benign and remains a successful result.
        let duplicate = adapter.submit_extracted(&items, None);
        assert!(duplicate[0].is_ok());
        assert!(duplicate[0].as_ref().unwrap().starts_with("queued "));
    }

    #[cfg(feature = "noema")]
    #[test]
    #[serial_test::serial]
    fn auto_extract_applies_autosafe_write_policy_to_root() {
        use noema_core::api::NoemaEngine;
        use noema_core::config::WritePolicy;
        use noema_core::sensitivity::Principal;

        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("NOEMA_ROOT", dir.path());
        // auto_extract on → effective policy is autoSafe, applied at startup.
        let settings = NoemaSettings {
            auto_extract: Some(true),
            user: Some("kay".into()),
            ..Default::default()
        };
        let _adapter = ZodeNoema::from_settings(&settings);
        std::env::remove_var("NOEMA_ROOT");

        let engine = NoemaEngine::new(dir.path()).unwrap();
        let view = engine
            .policy_get(&Principal::personal("kay".to_string(), "zode"))
            .unwrap();
        assert_eq!(view.write, WritePolicy::AutoSafe);
    }

    #[cfg(feature = "noema")]
    #[test]
    #[serial_test::serial]
    fn explicit_auto_extract_off_keeps_review_policy() {
        use noema_core::api::NoemaEngine;
        use noema_core::config::WritePolicy;
        use noema_core::sensitivity::Principal;

        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("NOEMA_ROOT", dir.path());
        // auto_extract explicitly off → policy reverts to review (extraction
        // defaults ON now, so the off case must be opted into).
        let settings = NoemaSettings {
            auto_extract: Some(false),
            user: Some("kay".into()),
            ..Default::default()
        };
        let _adapter = ZodeNoema::from_settings(&settings);
        std::env::remove_var("NOEMA_ROOT");

        let engine = NoemaEngine::new(dir.path()).unwrap();
        let view = engine
            .policy_get(&Principal::personal("kay".to_string(), "zode"))
            .unwrap();
        assert_eq!(view.write, WritePolicy::Review);
    }

    #[cfg(feature = "noema")]
    #[test]
    #[serial_test::serial]
    fn settings_default_to_noema_root_env_and_user_override() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("NOEMA_ROOT", dir.path());
        let settings = NoemaSettings {
            user: Some("configured-user".into()),
            ..Default::default()
        };
        let adapter = ZodeNoema::from_settings(&settings);
        std::env::remove_var("NOEMA_ROOT");

        assert_eq!(adapter.root(), Some(dir.path()));
        assert_eq!(adapter.user(), Some("configured-user"));
        assert!(adapter.auto_remember_enabled());
    }

    #[cfg(feature = "noema")]
    #[test]
    fn remember_then_recall_round_trips_user_memory() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = ZodeNoema::from_root_with_user(dir.path(), Some("kay".to_string()));
        let stored = adapter
            .remember(
                "Prefer Rust for Noema integration work.",
                ZodeMemoryScope::User,
                None,
            )
            .unwrap();
        assert!(stored.contains("remembered mem_"), "{stored}");

        let recalled = adapter
            .recall_for_turn("rust integration", None)
            .unwrap()
            .expect("memory recalled");
        assert!(recalled.contains("Prefer Rust for Noema integration work."));
    }

    #[cfg(feature = "noema")]
    #[test]
    fn empty_recall_pack_is_not_injected() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = ZodeNoema::from_root_with_user(dir.path(), Some("kay".to_string()));
        assert!(adapter
            .recall_for_turn("nothing here", None)
            .unwrap()
            .is_none());
    }

    #[cfg(feature = "noema")]
    #[test]
    fn auto_remember_round_trips_explicit_user_request() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = ZodeNoema::from_root_with_user(dir.path(), Some("kay".to_string()));
        let stored = adapter
            .auto_remember_from_turn("请记住我喜欢 Rust 工具", None)
            .unwrap()
            .expect("explicit memory stored");
        assert!(stored.contains("remembered mem_"), "{stored}");

        let recalled = adapter
            .recall_for_turn("Rust 工具", None)
            .unwrap()
            .expect("auto memory recalled");
        assert!(recalled.contains("我喜欢 Rust 工具"), "{recalled}");
    }

    #[cfg(feature = "noema")]
    #[test]
    fn auto_remember_round_trips_chinese_preference_statement() {
        let dir = tempfile::tempdir().unwrap();
        let adapter = ZodeNoema::from_root_with_user(dir.path(), Some("kay".to_string()));
        let stored = adapter
            .auto_remember_from_turn("王小明爱吃酸的", None)
            .unwrap()
            .expect("stable preference statement stored");
        assert!(stored.contains("remembered mem_"), "{stored}");

        let recalled = adapter
            .recall_for_turn("王小明爱吃什么", None)
            .unwrap()
            .expect("auto memory recalled");
        assert!(recalled.contains("王小明爱吃酸的"), "{recalled}");
    }
}
