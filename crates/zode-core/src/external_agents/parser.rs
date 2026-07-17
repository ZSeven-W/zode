//! Stream parsers for external agent stdout. Display-plane events NEVER hard
//! fail (unknown lines degrade to `ExtEvent::Log`); control-plane fields
//! (final result, session id for JSONL dialects) missing at end-of-stream IS
//! a hard failure — hire/resume semantics would dangle otherwise.

use super::capability::OutputProtocol;

/// One display-plane event, forwarded to the sub-agent progress UI.
#[derive(Debug, Clone, PartialEq)]
pub enum ExtEvent {
    Text(String),
    ToolUse { name: String, summary: String },
    Log(String),
}

/// Control-plane outcome extracted at end-of-stream.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FinalResult {
    pub text: String,
    pub session_id: Option<String>,
    pub usage_in: Option<u64>,
    pub usage_out: Option<u64>,
    pub model: Option<String>,
}

#[derive(Debug)]
pub struct StreamParser {
    protocol: OutputProtocol,
    text_lines: Vec<String>,
    result: Option<FinalResult>,
    session_id: Option<String>,
    model: Option<String>,
    usage: Option<(u64, u64)>,
    /// codex dialect: last agent_message becomes the result text.
    last_agent_message: Option<String>,
    saw_terminal: bool,
}

impl StreamParser {
    pub fn new(protocol: &OutputProtocol) -> Self {
        Self {
            protocol: *protocol,
            text_lines: Vec::new(),
            result: None,
            session_id: None,
            model: None,
            usage: None,
            last_agent_message: None,
            saw_terminal: false,
        }
    }

    /// Feed one stdout line. Never fails; unknown content degrades to Log.
    pub fn feed(&mut self, line: &str) -> Vec<ExtEvent> {
        match self.protocol {
            OutputProtocol::Text => {
                self.text_lines.push(line.to_string());
                vec![ExtEvent::Text(line.to_string())]
            }
            OutputProtocol::JsonlClaude => self.feed_claude(line),
            OutputProtocol::JsonlCodex => self.feed_codex(line),
        }
    }

    fn feed_claude(&mut self, line: &str) -> Vec<ExtEvent> {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            return vec![ExtEvent::Log(line.to_string())];
        };
        if let Some(sid) = v.get("session_id").and_then(|s| s.as_str()) {
            self.session_id = Some(sid.to_string());
        }
        if let Some(m) = v.get("model").and_then(|s| s.as_str()) {
            self.model = Some(m.to_string());
        }
        match v.get("type").and_then(|t| t.as_str()) {
            Some("assistant") => {
                let mut events = Vec::new();
                let blocks = v
                    .pointer("/message/content")
                    .and_then(|c| c.as_array())
                    .cloned()
                    .unwrap_or_default();
                for b in blocks {
                    match b.get("type").and_then(|t| t.as_str()) {
                        Some("text") => {
                            if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                                events.push(ExtEvent::Text(t.to_string()));
                            }
                        }
                        Some("tool_use") => {
                            let name = b
                                .get("name")
                                .and_then(|n| n.as_str())
                                .unwrap_or("tool")
                                .to_string();
                            let summary = b
                                .get("input")
                                .map(|i| {
                                    let s = i.to_string();
                                    s.chars().take(120).collect::<String>()
                                })
                                .unwrap_or_default();
                            events.push(ExtEvent::ToolUse { name, summary });
                        }
                        _ => {}
                    }
                }
                if events.is_empty() {
                    events.push(ExtEvent::Log(line.to_string()));
                }
                events
            }
            Some("result") => {
                self.saw_terminal = true;
                let usage_in = v.pointer("/usage/input_tokens").and_then(|u| u.as_u64());
                let usage_out = v.pointer("/usage/output_tokens").and_then(|u| u.as_u64());
                if usage_in.is_some() || usage_out.is_some() {
                    self.usage = Some((usage_in.unwrap_or(0), usage_out.unwrap_or(0)));
                }
                self.result = Some(FinalResult {
                    text: v
                        .get("result")
                        .and_then(|r| r.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    session_id: self.session_id.clone(),
                    usage_in,
                    usage_out,
                    model: self.model.clone(),
                });
                vec![]
            }
            _ => vec![ExtEvent::Log(line.to_string())],
        }
    }

    fn feed_codex(&mut self, line: &str) -> Vec<ExtEvent> {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            return vec![ExtEvent::Log(line.to_string())];
        };
        if let Some(sid) = v
            .get("thread_id")
            .or_else(|| v.get("session_id"))
            .and_then(|s| s.as_str())
        {
            self.session_id = Some(sid.to_string());
        }
        match v.get("type").and_then(|t| t.as_str()) {
            Some("item.completed") => {
                let item = v.get("item").cloned().unwrap_or_default();
                match item.get("type").and_then(|t| t.as_str()) {
                    Some("agent_message") => {
                        let text = item
                            .get("text")
                            .and_then(|t| t.as_str())
                            .unwrap_or_default()
                            .to_string();
                        self.last_agent_message = Some(text.clone());
                        vec![ExtEvent::Text(text)]
                    }
                    Some(kind) => {
                        let summary = item
                            .get("command")
                            .or_else(|| item.get("path"))
                            .and_then(|c| c.as_str())
                            .unwrap_or_default()
                            .to_string();
                        vec![ExtEvent::ToolUse {
                            name: kind.to_string(),
                            summary,
                        }]
                    }
                    None => vec![ExtEvent::Log(line.to_string())],
                }
            }
            Some("turn.completed") => {
                self.saw_terminal = true;
                let usage_in = v.pointer("/usage/input_tokens").and_then(|u| u.as_u64());
                let usage_out = v.pointer("/usage/output_tokens").and_then(|u| u.as_u64());
                self.result = Some(FinalResult {
                    text: self.last_agent_message.clone().unwrap_or_default(),
                    session_id: self.session_id.clone(),
                    usage_in,
                    usage_out,
                    model: self.model.clone(),
                });
                vec![]
            }
            _ => vec![ExtEvent::Log(line.to_string())],
        }
    }

    /// End-of-stream. Text protocol always succeeds; JSONL dialects hard-fail
    /// when the terminal control-plane event never arrived.
    pub fn finish(self) -> Result<FinalResult, String> {
        match self.protocol {
            OutputProtocol::Text => Ok(FinalResult {
                text: self.text_lines.join("\n"),
                session_id: None,
                usage_in: None,
                usage_out: None,
                model: None,
            }),
            OutputProtocol::JsonlClaude | OutputProtocol::JsonlCodex => {
                if !self.saw_terminal {
                    return Err(
                        "external agent stream ended without a terminal result event".to_string(),
                    );
                }
                self.result
                    .ok_or_else(|| "terminal event seen but no result captured".to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_stream_parses_events_and_result() {
        let mut p = StreamParser::new(&OutputProtocol::JsonlClaude);
        let mut events = vec![];
        for line in include_str!("../../tests/fixtures/extagent/claude-stream.jsonl").lines() {
            events.extend(p.feed(line));
        }
        assert!(events
            .iter()
            .any(|e| matches!(e, ExtEvent::ToolUse { name, .. } if name == "Edit")));
        let r = p.finish().unwrap();
        assert_eq!(r.session_id.as_deref(), Some("sess-0001"));
        assert_eq!(r.text, "完成：已更新 src/a.rs");
        assert_eq!((r.usage_in, r.usage_out), (Some(120), Some(45)));
        assert_eq!(r.model.as_deref(), Some("claude-x"));
    }

    #[test]
    fn codex_stream_parses_events_and_result() {
        let mut p = StreamParser::new(&OutputProtocol::JsonlCodex);
        let mut events = vec![];
        for line in include_str!("../../tests/fixtures/extagent/codex-stream.jsonl").lines() {
            events.extend(p.feed(line));
        }
        assert!(events
            .iter()
            .any(|e| matches!(e, ExtEvent::ToolUse { name, .. } if name == "command_execution")));
        let r = p.finish().unwrap();
        assert_eq!(r.text, "测试全部通过");
        assert_eq!(r.session_id.as_deref(), Some("th-0001"));
        assert_eq!((r.usage_in, r.usage_out), (Some(80), Some(30)));
    }

    /// Ground-truth: this fixture is verbatim stdout captured from a real
    /// `codex exec --json` run (codex-cli 0.144.1). It exercises lines the
    /// hand-written fixture omits — `turn.started`, the `item.completed`
    /// shape with a nested `id`, and the richer `usage` object with extra
    /// keys — proving the parser handles the actual CLI output, not just an
    /// idealized shape.
    #[test]
    fn codex_real_cli_output_parses() {
        let mut p = StreamParser::new(&OutputProtocol::JsonlCodex);
        for line in include_str!("../../tests/fixtures/extagent/codex-real-stream.jsonl").lines() {
            p.feed(line);
        }
        let r = p.finish().unwrap();
        assert_eq!(r.text, "pong");
        assert_eq!(
            r.session_id.as_deref(),
            Some("019f6ee6-5f5e-7873-a1be-2f5308cfb0a4")
        );
        // `turn.completed.usage` carries extra keys (cached_input_tokens,
        // reasoning_output_tokens) — we take the two we need and ignore rest.
        assert_eq!((r.usage_in, r.usage_out), (Some(14981), Some(5)));
    }

    #[test]
    fn unknown_lines_degrade_to_log_and_missing_result_is_hard_error() {
        let mut p = StreamParser::new(&OutputProtocol::JsonlClaude);
        assert!(matches!(p.feed("not json").as_slice(), [ExtEvent::Log(_)]));
        assert!(p.finish().is_err(), "control-plane missing must hard-fail");
    }

    #[test]
    fn text_protocol_accumulates_stdout() {
        let mut p = StreamParser::new(&OutputProtocol::Text);
        p.feed("line one");
        p.feed("line two");
        let r = p.finish().unwrap();
        assert_eq!(r.text, "line one\nline two");
    }
}
