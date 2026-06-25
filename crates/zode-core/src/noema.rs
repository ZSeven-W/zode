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
            Self {
                root: Some(root),
                user,
                auto_remember: settings.auto_remember(),
            }
        }

        #[cfg(not(feature = "noema"))]
        {
            Self::disabled()
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
                NoemaEngine, RememberRequest, ReviewAction, ReviewDecisionRequest, ReviewOutcome,
                SubmitOutcome,
            };
            use noema_core::memory::{MemoryKind, Scope};
            use noema_core::sensitivity::SensitivityLevel;

            let engine = NoemaEngine::new(root).map_err(|err| err.to_string())?;
            let noema_scope = match scope {
                ZodeMemoryScope::User => Scope::User,
                ZodeMemoryScope::Project => Scope::Project,
            };
            let project_path = matches!(scope, ZodeMemoryScope::Project)
                .then(|| cwd.map(Path::to_path_buf))
                .flatten();
            // The old direct-write remember_text was removed to close the governance
            // bypass, so we go through submit_candidate. An explicit user "remember"
            // should persist immediately (the review queue is for auto-extracted
            // candidates, not explicit requests), so if it queues we accept it now.
            let outcome = engine
                .submit_candidate(RememberRequest {
                    principal: self.principal(),
                    text: text.to_string(),
                    scope: noema_scope,
                    project_path,
                    kind: MemoryKind::Preference,
                    sensitivity: SensitivityLevel::Internal,
                    tags: Vec::new(),
                    entities: Vec::new(),
                    confidence: 1.0,
                    importance: 0.5,
                })
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
