use std::collections::HashMap;

use agent::stream::Event;
use zode_node_protocol::{AgentEventKind, ToolCall, ToolStatus, UsageSnapshot};

const UNKNOWN_EVENT_CODE: &str = "agent.event.unknown";
const UNKNOWN_EVENT_MESSAGE: &str = "Ignored an unsupported agent runtime event";
const UNKNOWN_TOOL_NAME: &str = "unknown";
const UNKNOWN_TOOL_SUMMARY: &str = "Tool result";
const MAX_SUMMARY_CHARS: usize = 160;

#[derive(Debug, Clone)]
struct CachedTool {
    name: String,
    summary: String,
}

/// Converts agent-runtime stream events into the stable node protocol.
///
/// Tool arguments and results stay behind this boundary. Only a small,
/// display-safe summary is cached so a later `ToolResult` can reuse the tool's
/// identity without exposing its raw payload.
#[derive(Debug, Default)]
pub struct EventNormalizer {
    tools: HashMap<String, CachedTool>,
}

impl EventNormalizer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn normalize(&mut self, event: Event) -> Option<AgentEventKind> {
        match event {
            Event::TextDelta { delta } => Some(AgentEventKind::TextDelta { delta }),
            Event::Thinking { delta } => Some(AgentEventKind::ThinkingDelta { delta }),
            Event::ToolUse { id, name, input } => {
                let summary = safe_tool_summary(&name, &input);
                self.tools.insert(
                    id.clone(),
                    CachedTool {
                        name: name.clone(),
                        summary: summary.clone(),
                    },
                );

                Some(AgentEventKind::ToolStarted {
                    tool: ToolCall {
                        id,
                        name,
                        status: ToolStatus::Running,
                        summary,
                        detail: None,
                    },
                })
            }
            Event::ToolResult { id, ok, .. } => {
                let cached = self.tools.remove(&id).unwrap_or_else(|| CachedTool {
                    name: UNKNOWN_TOOL_NAME.to_owned(),
                    summary: UNKNOWN_TOOL_SUMMARY.to_owned(),
                });

                Some(AgentEventKind::ToolCompleted {
                    tool: ToolCall {
                        id,
                        name: cached.name,
                        status: if ok {
                            ToolStatus::Completed
                        } else {
                            ToolStatus::Failed
                        },
                        summary: cached.summary,
                        detail: None,
                    },
                })
            }
            Event::Usage {
                input_tokens,
                output_tokens,
                ..
            } => Some(AgentEventKind::Usage {
                usage: UsageSnapshot {
                    input_tokens: u64::from(input_tokens),
                    output_tokens: u64::from(output_tokens),
                    context_used: None,
                    cost_usd: None,
                },
            }),
            Event::Notice { code, message } => Some(AgentEventKind::StatusNotice { code, message }),
            Event::Error { message, .. } => Some(AgentEventKind::Error {
                message,
                retryable: true,
            }),
            Event::Result { .. } => None,
            Event::Unknown => Some(unknown_event_notice()),
            _ => Some(unknown_event_notice()),
        }
    }
}

fn safe_tool_summary(name: &str, input: &serde_json::Value) -> String {
    let Some(input) = input.as_object() else {
        return name.to_owned();
    };

    for key in ["path", "url", "query"] {
        if let Some(value) = input.get(key).and_then(serde_json::Value::as_str) {
            let value = sanitize_summary_value(value);
            if !value.is_empty() {
                return format!("{name} {key}={value}");
            }
        }
    }

    name.to_owned()
}

fn sanitize_summary_value(value: &str) -> String {
    let normalized = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>();
    let normalized = normalized.split_whitespace().collect::<Vec<_>>().join(" ");

    if normalized.chars().count() <= MAX_SUMMARY_CHARS {
        normalized
    } else {
        format!(
            "{}…",
            normalized
                .chars()
                .take(MAX_SUMMARY_CHARS)
                .collect::<String>()
        )
    }
}

fn unknown_event_notice() -> AgentEventKind {
    AgentEventKind::StatusNotice {
        code: UNKNOWN_EVENT_CODE.to_owned(),
        message: UNKNOWN_EVENT_MESSAGE.to_owned(),
    }
}
