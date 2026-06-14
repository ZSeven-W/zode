//! Interactive question channel — the agent↔UI bridge for the
//! `AskUserQuestion` tool. Mirrors [`crate::approval`]'s queue/oneshot pattern
//! but is a SEPARATE channel: a question is not an approval (it has no
//! allow/deny semantics and its own dialog), so keeping it parallel avoids
//! entangling the well-tested permission path.
//!
//! Flow: the tool calls [`QuestionQueue::ask`], which sends a
//! [`QuestionRequest`] (carrying a oneshot) to the UI and awaits the chosen
//! option index. The UI drains [`QuestionReceiver`] in its event loop, shows a
//! picker, and calls [`QuestionRequest::respond`] with the selection (or `None`
//! when dismissed). A closed channel / dropped responder resolves to `None`.

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};

/// One pending question flowing from the `AskUserQuestion` tool to the UI.
#[derive(Debug)]
pub struct QuestionRequest {
    pub question: String,
    pub options: Vec<String>,
    /// Source tab id, so the UI can focus the asking conversation.
    pub source: Option<String>,
    sender: oneshot::Sender<Option<usize>>,
}

impl QuestionRequest {
    /// Send the chosen option index back to the waiting tool (`None` = the user
    /// dismissed the question). Err(choice) if the asker already gave up.
    pub fn respond(self, choice: Option<usize>) -> Result<(), Option<usize>> {
        self.sender.send(choice)
    }
}

/// Tool-facing handle (cheap to clone).
#[derive(Debug, Clone)]
pub struct QuestionQueue {
    sender: mpsc::UnboundedSender<QuestionRequest>,
}

/// UI-facing handle — single consumer; drain in the TUI select! loop.
#[derive(Debug)]
pub struct QuestionReceiver {
    receiver: mpsc::UnboundedReceiver<QuestionRequest>,
}

impl QuestionReceiver {
    pub async fn next(&mut self) -> Option<QuestionRequest> {
        self.receiver.recv().await
    }
}

pub fn question_queue() -> (QuestionQueue, QuestionReceiver) {
    let (tx, rx) = mpsc::unbounded_channel();
    (
        QuestionQueue { sender: tx },
        QuestionReceiver { receiver: rx },
    )
}

impl QuestionQueue {
    /// Ask the user to pick one of `options`; returns the chosen index, or
    /// `None` if dismissed / no UI is draining the channel.
    pub async fn ask(
        &self,
        question: &str,
        options: &[String],
        source: Option<String>,
    ) -> Option<usize> {
        let (tx, rx) = oneshot::channel();
        let req = QuestionRequest {
            question: question.to_string(),
            options: options.to_vec(),
            source,
            sender: tx,
        };
        if self.sender.send(req).is_err() {
            return None;
        }
        rx.await.unwrap_or(None)
    }
}

/// The `AskUserQuestion` tool: pauses the turn to ask the user a single-choice
/// question and returns their selection. Read-only (it changes nothing), so it
/// is not wrapped by the permission gate. Registered only when a UI is present
/// to answer it (a `QuestionQueue` is wired).
#[derive(Debug)]
pub struct AskUserQuestionTool {
    queue: QuestionQueue,
    source: Option<String>,
}

impl AskUserQuestionTool {
    pub fn new(queue: QuestionQueue, source: Option<String>) -> Self {
        Self { queue, source }
    }

    fn parse(input: &Value) -> Result<(String, Vec<String>), AgentError> {
        let question = input
            .get("question")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| AgentError::other("AskUserQuestion: missing string 'question'"))?;
        let options: Vec<String> = input
            .get("options")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        if options.len() < 2 {
            return Err(AgentError::other(
                "AskUserQuestion: 'options' must have at least 2 choices",
            ));
        }
        Ok((question, options))
    }
}

#[async_trait]
impl Tool for AskUserQuestionTool {
    fn name(&self) -> &str {
        "AskUserQuestion"
    }
    fn description(&self) -> &str {
        "Ask the user a single-choice question and wait for their answer. Use only when a decision is genuinely the user's to make (ambiguous requirements, a fork you can't resolve from the code) — not for things you can decide yourself."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["question", "options"],
            "properties": {
                "question": { "type": "string", "description": "The question to ask." },
                "options": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 2,
                    "maxItems": 6,
                    "description": "The choices to offer (2-6)."
                }
            }
        })
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::ReadOnly
    }
    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let (question, options) = Self::parse(&input)?;
        match self.queue.ask(&question, &options, self.source.clone()).await {
            Some(i) if i < options.len() => {
                Ok(json!({ "selected": options[i], "index": i }))
            }
            // Dismissed (Esc) or no UI: report it so the agent can proceed
            // sensibly rather than treating the turn as failed.
            _ => Ok(json!({ "selected": null, "dismissed": true })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn ask_round_trips_the_selected_index() {
        let (queue, mut rx) = question_queue();
        tokio::spawn(async move {
            if let Some(req) = rx.next().await {
                assert_eq!(req.question, "pick one");
                assert_eq!(req.options.len(), 3);
                let _ = req.respond(Some(2));
            }
        });
        let choice = queue
            .ask("pick one", &["a".into(), "b".into(), "c".into()], None)
            .await;
        assert_eq!(choice, Some(2));
    }

    #[tokio::test]
    async fn closed_channel_resolves_to_none() {
        let (queue, rx) = question_queue();
        drop(rx);
        assert_eq!(queue.ask("q", &["a".into(), "b".into()], None).await, None);
    }

    #[tokio::test]
    async fn tool_returns_selection_and_handles_dismiss() {
        let (queue, mut rx) = question_queue();
        // UI answers the first question with index 0, dismisses the second.
        tokio::spawn(async move {
            if let Some(req) = rx.next().await {
                let _ = req.respond(Some(0));
            }
            if let Some(req) = rx.next().await {
                let _ = req.respond(None);
            }
        });
        let tool = AskUserQuestionTool::new(queue, None);
        let ctx = ToolUseContext::new(std::env::temp_dir());
        let input = json!({ "question": "color?", "options": ["red", "blue"] });

        let answered = tool.call(&ctx, input.clone()).await.unwrap();
        assert_eq!(answered["selected"], "red");
        assert_eq!(answered["index"], 0);

        let dismissed = tool.call(&ctx, input).await.unwrap();
        assert_eq!(dismissed["dismissed"], true);
        assert!(dismissed["selected"].is_null());
    }

    #[tokio::test]
    async fn tool_rejects_too_few_options() {
        let (queue, _rx) = question_queue();
        let tool = AskUserQuestionTool::new(queue, None);
        let ctx = ToolUseContext::new(std::env::temp_dir());
        let err = tool
            .call(&ctx, json!({ "question": "q", "options": ["only"] }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("at least 2"));
    }
}
