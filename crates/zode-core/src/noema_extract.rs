//! LLM-backed post-turn memory extraction.
//!
//! After a turn completes, zode runs a cheap one-shot LLM pass over the recent
//! transcript to pull *durable* memories (preferences, decisions, constraints,
//! conventions) — not just the explicit "remember…" phrases the string
//! heuristic in [`crate::noema`] catches. The model returns a JSON array; we
//! parse it here into [`ExtractedCandidate`]s, then [`crate::noema::ZodeNoema`]
//! submits each through noema's existing review/dedup/governance pipeline.
//!
//! This module is intentionally split from `noema.rs`: the parsing + transcript
//! shaping is pure and unit-testable with no engine, no network, no `noema`
//! feature. The async LLM call + candidate submission live in `noema.rs` and
//! `engine.rs`.

use crate::noema::ZodeMemoryScope;

/// Resolved runtime knobs for the post-turn extraction pass, derived from
/// [`crate::config::NoemaSettings`] at engine-assembly time. Kept feature-free
/// and `Clone` so a detached task can own a copy.
#[derive(Debug, Clone)]
pub struct ExtractConfig {
    /// Master gate — when false, no extraction runs (zero cost).
    pub enabled: bool,
    /// Model id for the extraction call. Empty → caller falls back to the
    /// engine's active model.
    pub model: Option<String>,
    /// Whether to mine the assistant's reply too (default false = user only).
    pub scan_assistant: bool,
    /// Max candidates accepted per turn (bounds cost + dedup churn).
    pub max_memories_per_turn: usize,
    /// Truncate the transcript slice fed to the model to this many chars.
    pub max_input_chars: usize,
}

impl Default for ExtractConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: None,
            scan_assistant: false,
            max_memories_per_turn: 3,
            max_input_chars: 4000,
        }
    }
}

impl ExtractConfig {
    /// Build from [`crate::config::NoemaSettings`]. `enabled` requires BOTH the
    /// settings' `auto_extract` flag and that memory itself is on.
    pub fn from_settings(settings: &crate::config::NoemaSettings) -> Self {
        Self {
            enabled: settings.enabled() && settings.auto_extract(),
            model: settings
                .extract_model
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty()),
            scan_assistant: settings.extract_scan_assistant.unwrap_or(false),
            max_memories_per_turn: settings.max_memories_per_turn.unwrap_or(3).max(1) as usize,
            max_input_chars: settings.extract_max_input_chars.unwrap_or(4000).max(200) as usize,
        }
    }
}

/// One memory the extractor proposes, already mapped to the noema vocabulary.
/// A superset of noema-core's `ExtractedMemory { body, entities, tags }` — it
/// also carries the classification fields the model must *infer* (kind, scope,
/// sensitivity, importance, confidence) rather than the hardcoded defaults the
/// old explicit path used.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtractedCandidate {
    pub body: String,
    pub entities: Vec<String>,
    pub tags: Vec<String>,
    pub kind: MemoryKindHint,
    pub scope: ZodeMemoryScope,
    pub sensitivity: SensitivityHint,
    pub importance: f32,
    pub confidence: f32,
    pub code: Option<ExtractedCode>,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct ExtractedCode {
    pub paths: Vec<String>,
    pub symbols: Vec<String>,
    pub head_commit: Option<String>,
    pub ref_commits: Vec<String>,
    pub lang: Option<String>,
    pub error_sig: Option<String>,
    pub commands: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParseExtractionResult {
    pub items: Vec<ExtractedCandidate>,
    pub degradation_count: usize,
}

/// Subset of noema's `MemoryKind` the extractor may emit. Mapped to the real
/// enum in `noema.rs` (kept feature-free here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryKindHint {
    Preference,
    Decision,
    Constraint,
    Fact,
    Reference,
    Workflow,
    Warning,
    ErrorFix,
}

/// Auto-extraction never stores above `Internal`: anything the model flags more
/// sensitive is clamped down (so noema's personal-tenant guards never reject)
/// and `Secret`-looking content is dropped upstream by the prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensitivityHint {
    Public,
    Internal,
}

/// System prompt for the extraction pass. Instructs the model to emit ONLY a
/// JSON array and to stay quiet when nothing is durable.
pub const EXTRACT_SYSTEM_PROMPT: &str = r#"You extract DURABLE long-term memories from a coding-assistant conversation.

Return ONLY a JSON array (no prose, no code fences). Each element:
{
  "body": "<concise third-person statement of the durable fact/preference>",
  "entities": ["<proper nouns: tools, languages, people, projects>"],
  "tags": ["<short topical tags>"],
  "kind": "preference|decision|constraint|fact|reference|workflow|warning|error_fix",
  "scope": "user|project",
  "sensitivity": "public|internal",
  "importance": 0.0-1.0,
  "confidence": 0.0-1.0
}

Optional field "code": {"paths": [..], "symbols": [..], "error_sig": "...",
"commands": [..], "lang": "..", "ref_commits": [..]}.

Rules:
- Emit ONLY information worth remembering across future sessions: stable user
  preferences, project conventions/decisions, constraints, durable facts.
- Output [] when the turn has nothing durable (questions, transient chatter,
  one-off task details, debugging steps).
- Paths in "code.paths" must be relative to the CURRENT WORKING DIRECTORY of
  this session (or absolute) — copy them as they appear in the conversation;
  do not guess repo-root-relative forms. (The host converts them.)
- New kind "error_fix": a resolved error with a durable fix IS extractable —
  exception to the discard-debugging rule. Copy the leading diagnostic form into
  code.error_sig (e.g. "error[E0308]: ..." or "TypeError: ..."), never a
  paraphrase like "mismatched types". The body must be the fix statement, not a
  restatement of the error.
- NEVER store secrets, API keys, passwords, or tokens. Skip them entirely.
- "scope": user = about the person; project = about this codebase/repo.
- Set confidence < 0.8 when you are unsure; such items are queued for review
  rather than stored automatically.
- Keep each body one sentence. At most a few items."#;

/// Parse the model's JSON output into candidates. Pure and tolerant:
/// - accepts a bare array, a fenced ```json block, or a loose array span
///   (reuses [`crate::openpencil::design::extract_json`]);
/// - drops malformed / empty-body items;
/// - clamps `sensitivity` to ≤ `Internal` and `scope` to {user, project};
/// - clamps `importance`/`confidence` to `[0,1]`;
/// - truncates to `max` items (the per-turn cap).
///
/// Returns an empty result for `[]`, garbage, or a non-array top level.
pub fn parse_extraction(json_text: &str, max: usize) -> ParseExtractionResult {
    let Some(value) = crate::openpencil::design::extract_json(json_text) else {
        return ParseExtractionResult::default();
    };
    let Some(array) = value.as_array() else {
        return ParseExtractionResult::default();
    };
    let mut result = ParseExtractionResult::default();
    for item in array.iter().filter_map(parse_one).take(max) {
        result.degradation_count += item.1;
        result.items.push(item.0);
    }
    result
}

/// Shape the transcript slice fed to the extractor from the last user message
/// and (optionally) the assistant reply. Returns `None` when there is nothing
/// worth extracting (empty user text). Truncates to `max_chars` from the END
/// (the most recent content) so a long turn still fits the budget.
pub fn build_transcript_slice(
    user_text: &str,
    assistant_text: Option<&str>,
    scan_assistant: bool,
    max_chars: usize,
) -> Option<String> {
    let user_text = user_text.trim();
    if user_text.is_empty() {
        return None;
    }
    let mut out = format!("User: {user_text}");
    if scan_assistant {
        if let Some(reply) = assistant_text.map(str::trim).filter(|s| !s.is_empty()) {
            out.push_str(&format!("\n\nAssistant: {reply}"));
        }
    }
    Some(truncate_tail_chars(&out, max_chars))
}

/// Keep the last `max_chars` characters (char-boundary safe), prefixing an
/// ellipsis when content was dropped.
fn truncate_tail_chars(text: &str, max_chars: usize) -> String {
    let count = text.chars().count();
    if count <= max_chars {
        return text.to_string();
    }
    let skip = count - max_chars;
    let tail: String = text.chars().skip(skip).collect();
    format!("…{tail}")
}

fn parse_one(item: &serde_json::Value) -> Option<(ExtractedCandidate, usize)> {
    let obj = item.as_object()?;
    let body = obj.get("body").and_then(|v| v.as_str())?.trim().to_string();
    if body.is_empty() {
        return None;
    }
    let (code, degradation_count) = parse_code(obj.get("code"));
    Some((
        ExtractedCandidate {
            body,
            entities: string_list(obj.get("entities")),
            tags: string_list(obj.get("tags")),
            kind: parse_kind(obj.get("kind").and_then(|v| v.as_str())),
            scope: parse_scope(obj.get("scope").and_then(|v| v.as_str())),
            sensitivity: parse_sensitivity(obj.get("sensitivity").and_then(|v| v.as_str())),
            importance: parse_unit(obj.get("importance"), 0.5),
            confidence: parse_unit(obj.get("confidence"), 0.5),
            code,
        },
        degradation_count,
    ))
}

fn parse_code(value: Option<&serde_json::Value>) -> (Option<ExtractedCode>, usize) {
    let Some(value) = value else { return (None, 0) };
    let Some(obj) = value.as_object() else {
        return (None, 1);
    };
    let mut degradation_count = 0;
    let paths = tolerant_string_list(obj.get("paths"), &mut degradation_count);
    let symbols = tolerant_string_list(obj.get("symbols"), &mut degradation_count);
    let ref_commits = tolerant_string_list(obj.get("ref_commits"), &mut degradation_count);
    let commands = tolerant_string_list(obj.get("commands"), &mut degradation_count);
    let head_commit = tolerant_string(obj.get("head_commit"), &mut degradation_count);
    let lang = tolerant_string(obj.get("lang"), &mut degradation_count);
    let error_sig = tolerant_string(obj.get("error_sig"), &mut degradation_count);
    (
        Some(ExtractedCode {
            paths,
            symbols,
            head_commit,
            ref_commits,
            lang,
            error_sig,
            commands,
        }),
        degradation_count,
    )
}

fn tolerant_string_list(
    value: Option<&serde_json::Value>,
    degradations: &mut usize,
) -> Vec<String> {
    let Some(value) = value else {
        return Vec::new();
    };
    let Some(array) = value.as_array() else {
        *degradations += 1;
        return Vec::new();
    };
    let mut invalid = false;
    let strings = array
        .iter()
        .filter_map(|value| match value.as_str() {
            Some(value) => Some(value.trim().to_string()),
            None => {
                invalid = true;
                None
            }
        })
        .filter(|value| !value.is_empty())
        .collect();
    *degradations += usize::from(invalid);
    strings
}

fn tolerant_string(value: Option<&serde_json::Value>, degradations: &mut usize) -> Option<String> {
    let value = value?;
    let Some(value) = value.as_str() else {
        *degradations += 1;
        return None;
    };
    Some(value.trim().to_string()).filter(|value| !value.is_empty())
}

fn string_list(value: Option<&serde_json::Value>) -> Vec<String> {
    value
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Unknown / missing kind defaults to `Preference` (matches the old hardcoded
/// default), so a vague model never blocks a useful memory.
fn parse_kind(value: Option<&str>) -> MemoryKindHint {
    match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("decision") => MemoryKindHint::Decision,
        Some("constraint") => MemoryKindHint::Constraint,
        Some("fact") => MemoryKindHint::Fact,
        Some("reference") => MemoryKindHint::Reference,
        Some("workflow") => MemoryKindHint::Workflow,
        Some("warning") => MemoryKindHint::Warning,
        Some("error_fix") => MemoryKindHint::ErrorFix,
        _ => MemoryKindHint::Preference,
    }
}

/// Only `user` / `project` are valid for the personal tenant; `team`/`org`/
/// unknown clamp to `User`.
fn parse_scope(value: Option<&str>) -> ZodeMemoryScope {
    match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("project") => ZodeMemoryScope::Project,
        _ => ZodeMemoryScope::User,
    }
}

/// Clamp sensitivity to ≤ Internal. Anything above (confidential/restricted/
/// secret) or unknown collapses to `Internal` — the auto-store ceiling.
fn parse_sensitivity(value: Option<&str>) -> SensitivityHint {
    match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("public") => SensitivityHint::Public,
        _ => SensitivityHint::Internal,
    }
}

fn parse_unit(value: Option<&serde_json::Value>, default: f32) -> f32 {
    value
        .and_then(|v| v.as_f64())
        .map(|f| (f as f32).clamp(0.0, 1.0))
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_extraction_reads_array_and_clamps_count() {
        let json = r#"[
            {"body":"User prefers Rust","kind":"preference","scope":"user","confidence":0.9,"importance":0.6},
            {"body":"Project uses Bun","kind":"decision","scope":"project"},
            {"body":"Third item"},
            {"body":"Fourth item"},
            {"body":"Fifth item"}
        ]"#;
        let out = parse_extraction(json, 3).items;
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].body, "User prefers Rust");
        assert_eq!(out[0].kind, MemoryKindHint::Preference);
        assert_eq!(out[0].scope, ZodeMemoryScope::User);
        assert_eq!(out[0].confidence, 0.9);
        assert_eq!(out[1].scope, ZodeMemoryScope::Project);
        assert_eq!(out[1].kind, MemoryKindHint::Decision);
    }

    #[test]
    fn parse_extraction_tolerates_fenced_and_loose_json() {
        let fenced = "Here are the memories:\n```json\n[{\"body\":\"User likes ripgrep\"}]\n```\n";
        let out = parse_extraction(fenced, 5).items;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].body, "User likes ripgrep");
    }

    #[test]
    fn parse_extraction_returns_empty_for_empty_array_and_garbage() {
        assert!(parse_extraction("[]", 5).items.is_empty());
        assert!(parse_extraction("not json at all", 5).items.is_empty());
        // A non-array JSON top level yields nothing.
        assert!(parse_extraction(r#"{"body":"x"}"#, 5).items.is_empty());
    }

    #[test]
    fn parse_extraction_drops_empty_body_items() {
        let json = r#"[{"body":"   "}, {"entities":["x"]}, {"body":"kept"}]"#;
        let out = parse_extraction(json, 5).items;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].body, "kept");
    }

    #[test]
    fn parse_extraction_clamps_sensitivity_and_scope() {
        // Model over-reaches: "secret" sensitivity, "team" scope, importance > 1.
        let json = r#"[{"body":"X","sensitivity":"secret","scope":"team","importance":1.7,"confidence":-0.2}]"#;
        let out = parse_extraction(json, 5).items;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].sensitivity, SensitivityHint::Internal);
        assert_eq!(out[0].scope, ZodeMemoryScope::User);
        assert_eq!(out[0].importance, 1.0);
        assert_eq!(out[0].confidence, 0.0);
    }

    #[test]
    fn kind_scope_defaults_when_model_omits_fields() {
        let out = parse_extraction(r#"[{"body":"Just a body"}]"#, 5).items;
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, MemoryKindHint::Preference);
        assert_eq!(out[0].scope, ZodeMemoryScope::User);
        assert_eq!(out[0].sensitivity, SensitivityHint::Internal);
        assert_eq!(out[0].importance, 0.5);
        assert_eq!(out[0].confidence, 0.5);
        assert!(out[0].entities.is_empty());
    }

    #[test]
    fn public_sensitivity_is_preserved() {
        let out = parse_extraction(r#"[{"body":"X","sensitivity":"public"}]"#, 5).items;
        assert_eq!(out[0].sensitivity, SensitivityHint::Public);
    }

    #[test]
    fn entities_and_tags_trim_and_drop_blanks() {
        let json = r#"[{"body":"X","entities":["  ripgrep  ","",  " fd "],"tags":["search",""]}]"#;
        let out = parse_extraction(json, 5).items;
        assert_eq!(out[0].entities, vec!["ripgrep", "fd"]);
        assert_eq!(out[0].tags, vec!["search"]);
    }

    #[test]
    fn parse_extraction_with_code_and_error_fix_kind() {
        let json = r#"[{"body":"Convert with .to_string() before returning",
            "kind":"error_fix","scope":"project","importance":0.7,"confidence":0.9,
            "code":{"paths":["src/lib.rs"],"error_sig":"error[E0308]: mismatched types"}}]"#;
        let ParseExtractionResult {
            items,
            degradation_count,
        } = parse_extraction(json, 3);
        assert_eq!(degradation_count, 0);
        assert_eq!(items[0].code.as_ref().unwrap().paths, vec!["src/lib.rs"]);
        assert!(matches!(items[0].kind, MemoryKindHint::ErrorFix));
    }

    #[test]
    fn invalid_field_dropped_valid_siblings_survive() {
        let json = r#"[{"body":"b","kind":"fact","scope":"user","importance":0.5,
            "confidence":0.9,"code":{"paths":123,"symbols":["buildPack"]}}]"#;
        let ParseExtractionResult {
            items,
            degradation_count,
        } = parse_extraction(json, 3);
        let code = items[0].code.as_ref().unwrap();
        assert!(code.paths.is_empty());
        assert_eq!(code.symbols, vec!["buildPack"]);
        assert_eq!(degradation_count, 1);
    }

    #[test]
    fn malformed_code_degrades_to_no_anchor_never_rejects() {
        let json = r#"[{"body":"b","kind":"fact","scope":"user","importance":0.5,
            "confidence":0.9,"code":"not-an-object"}]"#;
        let ParseExtractionResult {
            items,
            degradation_count,
        } = parse_extraction(json, 3);
        assert_eq!(items.len(), 1);
        assert!(items[0].code.is_none());
        assert_eq!(degradation_count, 1);
    }

    #[test]
    fn no_detector_error_sig_degrades_predictably() {
        let json = r#"[{"body":"Convert before returning","kind":"error_fix",
            "code":{"error_sig":"mismatched types"}}]"#;
        let result = parse_extraction(json, 3);
        assert_eq!(result.degradation_count, 0);
        assert_eq!(
            result.items[0].code.as_ref().unwrap().error_sig.as_deref(),
            Some("mismatched types")
        );
    }

    #[test]
    fn build_transcript_slice_respects_scan_assistant() {
        // Default: user only.
        let s = build_transcript_slice("I like Rust", Some("Sure!"), false, 4000).unwrap();
        assert!(s.contains("User: I like Rust"));
        assert!(!s.contains("Assistant"));
        // scan_assistant on: include the reply.
        let s = build_transcript_slice("I like Rust", Some("Sure!"), true, 4000).unwrap();
        assert!(s.contains("Assistant: Sure!"));
        // Empty user text → nothing to extract.
        assert!(build_transcript_slice("   ", Some("x"), true, 4000).is_none());
    }

    #[test]
    fn build_transcript_slice_truncates_from_tail() {
        let long = "x".repeat(500);
        let s = build_transcript_slice(&long, None, false, 100).unwrap();
        assert!(s.chars().count() <= 101); // 100 + ellipsis
        assert!(s.starts_with('…'));
    }

    #[test]
    fn extract_config_from_settings_gates_and_defaults() {
        use crate::config::NoemaSettings;
        // On by default.
        let on = ExtractConfig::from_settings(&NoemaSettings::default());
        assert!(on.enabled);
        assert_eq!(on.max_memories_per_turn, 3);
        assert_eq!(on.max_input_chars, 4000);

        // Enabled requires auto_extract AND memory enabled.
        let explicit_on = ExtractConfig::from_settings(&NoemaSettings {
            auto_extract: Some(true),
            extract_model: Some("cheap-model".into()),
            max_memories_per_turn: Some(5),
            ..Default::default()
        });
        assert!(explicit_on.enabled);
        assert_eq!(explicit_on.model.as_deref(), Some("cheap-model"));
        assert_eq!(explicit_on.max_memories_per_turn, 5);

        // auto_extract on but memory disabled → still off.
        let memoff = ExtractConfig::from_settings(&NoemaSettings {
            enabled: Some(false),
            auto_extract: Some(true),
            ..Default::default()
        });
        assert!(!memoff.enabled);
    }
}
