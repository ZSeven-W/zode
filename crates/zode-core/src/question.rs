//! Interactive question channel — the agent↔UI bridge for the
//! `AskUserQuestion` tool. Mirrors [`crate::approval`]'s queue/oneshot pattern
//! but is a SEPARATE channel: a question is not an approval (it has no
//! allow/deny semantics and its own dialog), so keeping it parallel avoids
//! entangling the well-tested permission path.
//!
//! A tool request batches 4-8 questions (see [`MIN_QUESTIONS`]/[`MAX_QUESTIONS`]),
//! each single-choice over 2-10 preset options; the UI additionally always
//! offers a free-text "Other". (The internal `ask_specs`/`ask` API — used by the
//! OpenPencil consent prompt — has no such minimum.) The tool
//! gets back one answer per question (the chosen option label OR the custom
//! text), or `None` when the user dismisses.
//!
//! Flow: the tool calls [`QuestionQueue::ask_specs`], which sends a
//! [`QuestionRequest`] (carrying a oneshot) to the UI and awaits the answers.
//! The UI drains [`QuestionReceiver`] in its event loop, shows a picker, and
//! calls [`QuestionRequest::respond`]. A closed channel / dropped responder
//! resolves to `None`.

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::{mpsc, oneshot};

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};

/// Fewest questions the tool accepts in one request. Requiring a batch means a
/// model that would otherwise fire many one-question calls (which pile up in the
/// transcript) groups them into a single prompt instead.
pub const MIN_QUESTIONS: usize = 4;
/// Most questions allowed in a single request (the picker scrolls past a few).
pub const MAX_QUESTIONS: usize = 8;
/// Fewest preset options a question may offer.
pub const MIN_OPTIONS: usize = 2;
/// Most preset options a question may offer (the picker scrolls past a few).
pub const MAX_OPTIONS: usize = 10;

/// One question in a request: the prompt, an optional short header tag, and the
/// preset single-select options. The UI also offers a free-text "Other".
#[derive(Debug, Clone)]
pub struct QuestionSpec {
    pub question: String,
    pub header: Option<String>,
    pub options: Vec<String>,
}

/// One pending `AskUserQuestion` request flowing to the UI.
#[derive(Debug)]
pub struct QuestionRequest {
    pub specs: Vec<QuestionSpec>,
    /// Source tab id, so the UI can focus the asking conversation.
    pub source: Option<String>,
    sender: oneshot::Sender<Option<Vec<String>>>,
}

impl QuestionRequest {
    /// Send one answer per question back to the waiting tool (`None` = the user
    /// dismissed). Each answer is the chosen option label or a custom string.
    /// Err(answers) if the asker already gave up.
    pub fn respond(self, answers: Option<Vec<String>>) -> Result<(), Option<Vec<String>>> {
        self.sender.send(answers)
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
    /// Ask one or more questions; returns one answer per question (chosen option
    /// label or custom text), or `None` if dismissed / no UI is draining.
    pub async fn ask_specs(
        &self,
        specs: Vec<QuestionSpec>,
        source: Option<String>,
    ) -> Option<Vec<String>> {
        let (tx, rx) = oneshot::channel();
        let req = QuestionRequest {
            specs,
            source,
            sender: tx,
        };
        if self.sender.send(req).is_err() {
            return None;
        }
        rx.await.unwrap_or(None)
    }

    /// Single-question convenience for yes/no consent prompts: returns the
    /// chosen option index, or `None` if dismissed OR the user typed a custom
    /// answer that doesn't match a preset option.
    pub async fn ask(
        &self,
        question: &str,
        options: &[String],
        source: Option<String>,
    ) -> Option<usize> {
        let spec = QuestionSpec {
            question: question.to_string(),
            header: None,
            options: options.to_vec(),
        };
        let answer = self
            .ask_specs(vec![spec], source)
            .await?
            .into_iter()
            .next()?;
        options.iter().position(|o| o == &answer)
    }
}

/// The `AskUserQuestion` tool: pauses the turn to ask the user a batch of 4-8
/// single-choice questions (each with a free-text "Other") and returns their
/// answers.
/// Read-only (it changes nothing), so it is not wrapped by the permission gate.
/// Registered only when a UI is present to answer it.
#[derive(Debug)]
pub struct AskUserQuestionTool {
    queue: QuestionQueue,
    source: Option<String>,
}

impl AskUserQuestionTool {
    pub fn new(queue: QuestionQueue, source: Option<String>) -> Self {
        Self { queue, source }
    }

    /// Parse the multi-question shape `{ questions: [...] }`. A batch of at least
    /// [`MIN_QUESTIONS`] is required — the legacy single-question shape
    /// `{ question, options }` is rejected with guidance to batch, so a model
    /// asks its decisions together instead of one call at a time.
    fn parse(input: &Value) -> Result<Vec<QuestionSpec>, AgentError> {
        let arr = input
            .get("questions")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                AgentError::other(format!(
                    "AskUserQuestion: provide a 'questions' array with {MIN_QUESTIONS}-{MAX_QUESTIONS} \
                     entries — batch your decisions into one call rather than asking one at a time"
                ))
            })?;
        if arr.len() < MIN_QUESTIONS || arr.len() > MAX_QUESTIONS {
            return Err(AgentError::other(format!(
                "AskUserQuestion: 'questions' must have {MIN_QUESTIONS}-{MAX_QUESTIONS} entries \
                 (batch at least {MIN_QUESTIONS} decisions together)"
            )));
        }
        arr.iter().map(Self::parse_one).collect()
    }

    fn parse_one(v: &Value) -> Result<QuestionSpec, AgentError> {
        let question = v
            .get("question")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .ok_or_else(|| AgentError::other("AskUserQuestion: missing string 'question'"))?;
        let header = v
            .get("header")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .filter(|s| !s.is_empty());
        let options: Vec<String> = v
            .get("options")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if options.len() < MIN_OPTIONS {
            return Err(AgentError::other(format!(
                "AskUserQuestion: each question needs at least {MIN_OPTIONS} options"
            )));
        }
        if options.len() > MAX_OPTIONS {
            return Err(AgentError::other(format!(
                "AskUserQuestion: each question allows at most {MAX_OPTIONS} options"
            )));
        }
        Ok(QuestionSpec {
            question,
            header,
            options,
        })
    }
}

#[async_trait]
impl Tool for AskUserQuestionTool {
    fn name(&self) -> &str {
        "AskUserQuestion"
    }
    fn description(&self) -> &str {
        "Ask the user several single-choice questions AT ONCE and wait for the answers. \
         Batch at least 4 (up to 8) decisions into ONE call — do not call this repeatedly \
         with one question at a time. The user can also type a custom 'Other' answer for \
         any question. Returns { answers: [{ question, header, answer }] }, where `answer` \
         is the chosen option text or the user's custom text. Use only when a decision is \
         genuinely the user's to make (ambiguous requirements, a fork you can't resolve \
         from the code) — not for things you can decide yourself."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["questions"],
            "properties": {
                "questions": {
                    "type": "array",
                    "minItems": MIN_QUESTIONS,
                    "maxItems": MAX_QUESTIONS,
                    "description": "4-8 questions to ask together (batch your decisions; never one at a time). Each is single-choice; the UI always also offers a free-text 'Other'.",
                    "items": {
                        "type": "object",
                        "required": ["question", "options"],
                        "properties": {
                            "question": { "type": "string", "description": "The question to ask." },
                            "header": { "type": "string", "description": "Optional short tag (≤12 chars) shown as a chip." },
                            "options": {
                                "type": "array",
                                "items": { "type": "string" },
                                "minItems": MIN_OPTIONS,
                                "maxItems": MAX_OPTIONS,
                                "description": "The preset choices (2-10)."
                            }
                        }
                    }
                }
            }
        })
    }
    fn safety_class(&self) -> SafetyClass {
        SafetyClass::ReadOnly
    }
    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let specs = Self::parse(&input)?;
        match self
            .queue
            .ask_specs(specs.clone(), self.source.clone())
            .await
        {
            Some(answers) if answers.len() == specs.len() => {
                let out: Vec<Value> = specs
                    .iter()
                    .zip(answers)
                    .map(
                        |(s, a)| json!({ "question": s.question, "header": s.header, "answer": a }),
                    )
                    .collect();
                Ok(json!({ "answers": out }))
            }
            // Dismissed (Esc) or no UI: report it so the agent can proceed
            // sensibly rather than treating the turn as failed.
            _ => Ok(json!({ "answers": null, "dismissed": true })),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(q: &str, opts: &[&str]) -> QuestionSpec {
        QuestionSpec {
            question: q.to_string(),
            header: None,
            options: opts.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[tokio::test]
    async fn ask_specs_round_trips_per_question_answers() {
        let (queue, mut rx) = question_queue();
        tokio::spawn(async move {
            if let Some(req) = rx.next().await {
                assert_eq!(req.specs.len(), 2);
                let _ = req.respond(Some(vec!["blue".into(), "custom text".into()]));
            }
        });
        let answers = queue
            .ask_specs(
                vec![spec("color?", &["red", "blue"]), spec("name?", &["a", "b"])],
                None,
            )
            .await;
        assert_eq!(
            answers,
            Some(vec!["blue".to_string(), "custom text".to_string()])
        );
    }

    #[tokio::test]
    async fn ask_compat_maps_answer_to_index() {
        let (queue, mut rx) = question_queue();
        tokio::spawn(async move {
            if let Some(req) = rx.next().await {
                let _ = req.respond(Some(vec!["No".into()]));
            }
        });
        let choice = queue.ask("ok?", &["Yes".into(), "No".into()], None).await;
        assert_eq!(choice, Some(1));
    }

    #[tokio::test]
    async fn ask_compat_returns_none_for_custom_answer() {
        let (queue, mut rx) = question_queue();
        tokio::spawn(async move {
            if let Some(req) = rx.next().await {
                let _ = req.respond(Some(vec!["something else".into()]));
            }
        });
        let choice = queue.ask("ok?", &["Yes".into(), "No".into()], None).await;
        assert_eq!(choice, None);
    }

    #[tokio::test]
    async fn closed_channel_resolves_to_none() {
        let (queue, rx) = question_queue();
        drop(rx);
        assert_eq!(
            queue.ask_specs(vec![spec("q", &["a", "b"])], None).await,
            None
        );
    }

    /// A valid batch of four questions (the enforced minimum).
    fn four_questions() -> Value {
        json!({ "questions": [
            { "question": "color?", "options": ["red", "blue"] },
            { "question": "size?", "options": ["s", "l"] },
            { "question": "shape?", "options": ["round", "square"] },
            { "question": "speed?", "options": ["fast", "slow"] },
        ] })
    }

    #[tokio::test]
    async fn tool_returns_answers_and_handles_dismiss() {
        let (queue, mut rx) = question_queue();
        tokio::spawn(async move {
            if let Some(req) = rx.next().await {
                let _ = req.respond(Some(vec![
                    "red".into(),
                    "s".into(),
                    "round".into(),
                    "fast".into(),
                ]));
            }
            if let Some(req) = rx.next().await {
                let _ = req.respond(None);
            }
        });
        let tool = AskUserQuestionTool::new(queue, None);
        let ctx = ToolUseContext::new(std::env::temp_dir());
        let input = four_questions();

        let answered = tool.call(&ctx, input.clone()).await.unwrap();
        assert_eq!(answered["answers"][0]["answer"], "red");
        assert_eq!(answered["answers"][0]["question"], "color?");

        let dismissed = tool.call(&ctx, input).await.unwrap();
        assert_eq!(dismissed["dismissed"], true);
        assert!(dismissed["answers"].is_null());
    }

    #[tokio::test]
    async fn tool_rejects_legacy_single_question_shape() {
        // Batching is enforced: the old `{ question, options }` shape (and any
        // sub-minimum batch) is rejected with guidance to group decisions.
        let (queue, _rx) = question_queue();
        let tool = AskUserQuestionTool::new(queue, None);
        let ctx = ToolUseContext::new(std::env::temp_dir());
        let err = tool
            .call(&ctx, json!({ "question": "q", "options": ["a", "b"] }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("questions"), "got: {err}");
    }

    #[tokio::test]
    async fn tool_rejects_too_few_questions() {
        let (queue, _rx) = question_queue();
        let tool = AskUserQuestionTool::new(queue, None);
        let ctx = ToolUseContext::new(std::env::temp_dir());
        let q = json!({ "question": "q", "options": ["a", "b"] });
        let err = tool
            .call(&ctx, json!({ "questions": [q, q, q] })) // only 3 < MIN 4
            .await
            .unwrap_err();
        assert!(err.to_string().contains("4-8"), "got: {err}");
    }

    #[tokio::test]
    async fn tool_rejects_too_few_options() {
        let (queue, _rx) = question_queue();
        let tool = AskUserQuestionTool::new(queue, None);
        let ctx = ToolUseContext::new(std::env::temp_dir());
        // Four questions (passes the batch check) but one has a single option.
        let ok = json!({ "question": "q", "options": ["a", "b"] });
        let bad = json!({ "question": "q", "options": ["only"] });
        let err = tool
            .call(&ctx, json!({ "questions": [ok, ok, ok, bad] }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("at least 2"));
    }

    #[tokio::test]
    async fn tool_rejects_too_many_questions() {
        let (queue, _rx) = question_queue();
        let tool = AskUserQuestionTool::new(queue, None);
        let ctx = ToolUseContext::new(std::env::temp_dir());
        let q = json!({ "question": "q", "options": ["a", "b"] });
        let err = tool
            .call(&ctx, json!({ "questions": [q, q, q, q, q, q, q, q, q] })) // 9 > MAX 8
            .await
            .unwrap_err();
        assert!(err.to_string().contains("4-8"));
    }

    #[tokio::test]
    async fn tool_rejects_too_many_options() {
        let (queue, _rx) = question_queue();
        let tool = AskUserQuestionTool::new(queue, None);
        let ctx = ToolUseContext::new(std::env::temp_dir());
        let opts: Vec<&str> = vec!["1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11"];
        let ok = json!({ "question": "q", "options": ["a", "b"] });
        let bad = json!({ "question": "q", "options": opts });
        let err = tool
            .call(&ctx, json!({ "questions": [ok, ok, ok, bad] }))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("at most 10"));
    }
}
