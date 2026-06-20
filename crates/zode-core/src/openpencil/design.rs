//! Design-pipeline orchestrator: schedules op's design_skeleton/content/refine.

use std::sync::Arc;

use agent::provider::{Provider, StreamRequest};
use futures::StreamExt;
use serde_json::{json, Value};

use super::OpError;

/// A section: its skeleton spec (sent to design_skeleton) + the content intent
/// (consumed by the content step, NOT sent to skeleton).
#[derive(Debug, Clone)]
pub struct SectionPlan {
    pub skeleton: Value,
    pub content_intent: String,
}

/// The plan the content generator produces; maps to design_skeleton args.
#[derive(Debug, Clone)]
pub struct DesignPlan {
    pub root_frame: Value,
    pub sections: Vec<SectionPlan>,
    pub style_guide: Option<Value>,
    /// op's Rust parser reads canvasWidth as a positive integer.
    pub canvas_width: Option<u32>,
    pub page_id: Option<String>,
}

impl DesignPlan {
    /// Build `design_skeleton` args (`{rootFrame, sections, styleGuide?, canvasWidth?, pageId?}`).
    /// Per-section `content_intent` is NOT forwarded — it is consumed locally.
    pub fn to_skeleton_args(&self) -> Value {
        let mut args = json!({
            "rootFrame": self.root_frame,
            "sections": self.sections.iter().map(|s| s.skeleton.clone()).collect::<Vec<_>>(),
        });
        if let Some(sg) = &self.style_guide {
            args["styleGuide"] = sg.clone();
        }
        if let Some(w) = self.canvas_width {
            // Serialize as an integer, not a float — op's parser is strict.
            args["canvasWidth"] = json!(w);
        }
        if let Some(p) = &self.page_id {
            args["pageId"] = json!(p);
        }
        args
    }
}

/// Normalized design_skeleton result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skeleton {
    pub root_id: String,
    pub section_ids: Vec<String>,
}

/// Parse op's Rust design_skeleton result: `{rootId, sectionIds:"a,b,c", sections:"<json>"}`.
/// Returns `OpError::Parse` if `rootId` is absent.
pub fn normalize_skeleton(result: &Value) -> Result<Skeleton, OpError> {
    let root_id = result
        .get("rootId")
        .and_then(|v| v.as_str())
        .ok_or_else(|| OpError::Parse("design_skeleton: no rootId".into()))?
        .to_string();

    let section_ids = result
        .get("sectionIds")
        .and_then(|v| v.as_str())
        .map(|s| {
            s.split(',')
                .filter(|x| !x.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    Ok(Skeleton {
        root_id,
        section_ids,
    })
}

/// Outcome summary after the full design pipeline completes.
#[derive(Debug, Clone)]
pub struct DesignResult {
    pub section_ids: Vec<String>,
    pub refine: Value,
    pub failures: Vec<String>,
}

/// Lenient JSON extraction from model text: tries whole-string parse first,
/// then extracts from a ```json fence, then finds the first balanced {…}/[…] span.
pub fn extract_json(text: &str) -> Option<Value> {
    // Attempt 1: the whole string is valid JSON.
    if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
        return Some(v);
    }
    // Attempt 2: fenced code block (```json … ``` or ``` … ```).
    if let Some(start) = text
        .find("```json")
        .map(|i| i + 7)
        .or_else(|| text.find("```").map(|i| i + 3))
    {
        if let Some(end) = text[start..].find("```") {
            if let Ok(v) = serde_json::from_str::<Value>(text[start..start + end].trim()) {
                return Some(v);
            }
        }
    }
    // Attempt 3: first balanced {…} or […] span.
    for (open, close) in [('{', '}'), ('[', ']')] {
        if let (Some(s), Some(e)) = (text.find(open), text.rfind(close)) {
            if e > s {
                if let Ok(v) = serde_json::from_str::<Value>(&text[s..=e]) {
                    return Some(v);
                }
            }
        }
    }
    None
}

/// One-shot LLM completion: sends a single user turn under `system` and
/// collects the assistant's streamed text into a `String`.
pub async fn llm_oneshot(
    provider: &Arc<dyn Provider>,
    model: &str,
    system: &str,
    user: &str,
) -> Result<String, OpError> {
    use agent::abort::AbortController;
    use agent::message::{ContentBlock, Header, Message};
    use agent::stream::Event;

    let req = StreamRequest::new(
        model.to_string(),
        vec![Message::User {
            header: Header::new(),
            content: vec![ContentBlock::Text {
                text: user.to_string(),
            }],
        }],
    )
    .with_system(system.to_string());

    let mut stream = provider
        .stream(req, AbortController::new())
        .await
        .map_err(|e| OpError::Rpc(e.to_string()))?;

    let mut out = String::new();
    while let Some(item) = stream.next().await {
        match item {
            Ok(Event::TextDelta { delta }) => out.push_str(&delta),
            Ok(Event::Error { code, message }) => {
                return Err(OpError::Rpc(format!("stream error code={code}: {message}")));
            }
            Ok(_) => {} // Result / tool / thinking / usage / notice events ignored
            Err(e) => return Err(OpError::Rpc(e.to_string())),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn to_skeleton_args_matches_op_schema() {
        let plan = DesignPlan {
            root_frame: json!({"name":"root","width":1200,"height":800}),
            sections: vec![
                SectionPlan {
                    skeleton: json!({"name":"Header","height":80}),
                    content_intent: "logo + nav".into(),
                },
                SectionPlan {
                    skeleton: json!({"name":"Hero"}),
                    content_intent: "headline".into(),
                },
            ],
            style_guide: None,
            canvas_width: Some(1200),
            page_id: None,
        };
        let args = plan.to_skeleton_args();
        assert_eq!(args["rootFrame"]["width"], 1200);
        assert_eq!(args["sections"].as_array().unwrap().len(), 2);
        assert_eq!(args["sections"][0]["name"], "Header");
        assert_eq!(args["canvasWidth"], 1200);
        assert!(args.get("styleGuide").is_none()); // omitted when None
        assert!(args["sections"][0].get("content_intent").is_none()); // intent NOT sent to skeleton
    }

    #[test]
    fn normalize_skeleton_parses_rust_result() {
        let raw = json!({"rootId":"10","sectionIds":"11,12,13","sections":"[{\"id\":\"11\"}]"});
        let s = normalize_skeleton(&raw).unwrap();
        assert_eq!(s.root_id, "10");
        assert_eq!(s.section_ids, vec!["11", "12", "13"]);
    }

    #[test]
    fn normalize_skeleton_errors_without_root() {
        assert!(normalize_skeleton(&json!({"sectionIds":"1"})).is_err());
    }

    #[test]
    fn extract_json_handles_clean_fenced_and_prose() {
        assert_eq!(extract_json("{\"a\":1}").unwrap()["a"], 1);
        assert_eq!(extract_json("```json\n{\"a\":2}\n```").unwrap()["a"], 2);
        assert_eq!(
            extract_json("sure, here:\n{\"a\":3}\ndone").unwrap()["a"],
            3
        );
        assert_eq!(extract_json("[{\"x\":1}]").unwrap()[0]["x"], 1);
        assert!(extract_json("no json here").is_none());
    }
}
