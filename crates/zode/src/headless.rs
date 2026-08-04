//! Headless `-p/--print` mode: single turn streamed to stdout (plain,
//! `json`, or `stream-json`), plus the display helpers shared with the REPL
//! (`crate::repl`).

use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use agent::abort::AbortController;
use agent::stream::Event;
use futures::StreamExt;
use uuid::Uuid;
use zode_core::run_event::{RunEvent, RunEventContext, TurnOutcome, TurnRecorder};
use zode_core::sessions::{DurableSessionMeta, SessionStore};
use zode_core::ZodeEngine;

use crate::args::OutputFormat;
use crate::headless_output::{CostSummary, HeadlessAccumulator, HeadlessIds};

/// Run a single prompt, stream to stdout, return a process exit code.
/// Every print run owns a durable, V1-compatible session id, including fresh one-shots.
pub async fn run_print(
    engine: &ZodeEngine,
    prompt: &str,
    session_meta: DurableSessionMeta,
    output_format: OutputFormat,
) -> i32 {
    // A `-p` run over a large `--continue`/`--resume` context could exceed
    // the provider's input limit on the very first turn — compact it down
    // first (byte estimate; no prior Usage in a fresh process).
    engine.auto_compact_if_needed(None).await;
    let session_id = session_meta.id.clone();
    let turn_id = Uuid::new_v4().simple().to_string();
    let request_id = Uuid::new_v4().simple().to_string();
    let mut stdout = std::io::stdout();
    let mut recorder = TurnRecorder::new(
        SessionStore::open_default().ok(),
        RunEventContext::new(
            session_id.clone(),
            Some(turn_id.clone()),
            Some(request_id.clone()),
        ),
    );
    for envelope in recorder.start() {
        stream_json_line(&mut stdout, output_format, &envelope);
    }
    {
        let message_count = engine.store.lock().map(|store| store.len()).unwrap_or(0);
        if let Err(error) =
            recorder.begin_checkpoint(&engine.checkpoints, engine.cwd.clone(), message_count)
        {
            tracing::warn!("checkpoint start failed: {error}");
        }
    }

    let abort = AbortController::new();
    // Make Ctrl-C a first-class interrupt: abort the turn so the run reports
    // RunStatus::Interrupted / EXIT_INTERRUPTED instead of dying mid-write.
    {
        let abort = abort.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                abort.abort_with_reason("interrupted by Ctrl-C");
            }
        });
    }
    let mut accumulator = HeadlessAccumulator::default();
    let mut partial = false;
    let mut stream = match engine.turn(prompt, abort.clone()).await {
        Ok(stream) => Some(stream),
        Err(error) => {
            let message = error.to_string();
            accumulator.push_error("turn_start_error", message.clone());
            if output_format == OutputFormat::Plain {
                eprintln!("zode: {message}");
            }
            let envelope = recorder.record(RunEvent::Error {
                code: "turn_start_error".into(),
                message,
            });
            stream_json_line(&mut stdout, output_format, &envelope);
            None
        }
    };
    let turn_ran = stream.is_some();

    let mut tool_names = std::collections::HashMap::<String, String>::new();
    while let Some(item) = match stream.as_mut() {
        Some(stream) => stream.next().await,
        None => None,
    } {
        let event = match item {
            Ok(ev) => ev,
            Err(e) => {
                let message = e.to_string();
                accumulator.push_stream_error(message.clone());
                partial = true;
                if output_format == OutputFormat::Plain {
                    eprintln!("\nzode stream error: {message}");
                }
                let envelope = recorder.record(RunEvent::Error {
                    code: "stream_error".into(),
                    message,
                });
                stream_json_line(&mut stdout, output_format, &envelope);
                break;
            }
        };
        // Feed every event to the cost tracker (it counts only Usage events).
        engine.cost.observe(&event).await;
        accumulator.on_event(&engine.model, &event);
        let envelope = recorder.record_agent(&event);
        stream_json_line(&mut stdout, output_format, &envelope);
        match &event {
            Event::TextDelta { delta } if output_format == OutputFormat::Plain => {
                let _ = write!(stdout, "{delta}");
                let _ = stdout.flush();
            }
            Event::ToolUse { id, name, .. } => {
                tool_names.insert(id.clone(), name.clone());
                if output_format == OutputFormat::Plain {
                    eprintln!("· {name}");
                }
            }
            Event::ToolResult { id, ok, output } if output_format == OutputFormat::Plain => {
                if let Some(line) = tool_result_line(
                    tool_names.get(id).map(String::as_str),
                    *ok,
                    output,
                    Some(engine.cwd.as_path()),
                ) {
                    eprintln!("· {line}");
                }
            }
            Event::Error { code, message } if output_format == OutputFormat::Plain => {
                eprintln!("\nzode error [{code}]: {message}");
            }
            _ => {}
        }
    }
    if output_format == OutputFormat::Plain {
        let _ = writeln!(stdout);
    }
    // Mine the completed turn for durable memories (no-op unless autoExtract
    // is on). Awaited so the process doesn't exit before the write lands.
    engine.extract_post_turn_inline().await;
    // Every structured run is resumable, including a fresh one-shot.
    if let Err(error) = save_durable_session(engine, &session_meta).await {
        tracing::warn!("session save failed: {error}");
    }
    let cost =
        CostSummary::from_snapshot(&engine.cost.snapshot().await, engine.cost.currency_code());
    let result = accumulator.finish(
        HeadlessIds {
            session_id,
            turn_id,
            request_id,
            model: engine.model.clone(),
        },
        cost,
        partial,
        abort.is_aborted(),
    );
    if output_format == OutputFormat::Json {
        emit_json_line(&mut stdout, &result);
    }
    let outcome = TurnOutcome {
        status: result.status,
        stop_reason: result.stop_reason.clone(),
        partial: result.partial,
    };
    for envelope in recorder.complete(Some(&engine.checkpoints), turn_ran, &outcome) {
        stream_json_line(&mut stdout, output_format, &envelope);
    }
    if output_format == OutputFormat::Plain {
        // Token/cache usage to stderr (keeps stdout = the model's answer).
        eprintln!("{}", engine.cost.report().await);
    }
    result.exit_code()
}

fn stream_json_line(
    writer: &mut impl Write,
    output_format: OutputFormat,
    envelope: &zode_core::run_event::RunEventEnvelope,
) {
    if output_format == OutputFormat::StreamJson {
        emit_json_line(writer, envelope);
    }
}

fn emit_json_line(writer: &mut impl Write, value: &impl serde::Serialize) {
    match serde_json::to_writer(&mut *writer, value) {
        Ok(()) => {
            let _ = writer.write_all(b"\n");
            let _ = writer.flush();
        }
        Err(error) => eprintln!("zode: failed to encode structured output: {error}"),
    }
}

pub(crate) fn tool_result_line(
    name: Option<&str>,
    ok: bool,
    output: &serde_json::Value,
    cwd: Option<&Path>,
) -> Option<String> {
    let name = name.unwrap_or("tool");
    if !ok {
        return Some(format!("{name} failed: {}", compact_tool_payload(output)));
    }
    if output
        .get("passed")
        .and_then(|v| v.as_bool())
        .is_some_and(|passed| !passed)
    {
        return Some(format!("{name} failed: {}", compact_tool_payload(output)));
    }
    if let Some(summary) = file_mutation_result_location_summary(name, output, cwd) {
        return Some(format!("{name} done {summary}"));
    }
    if let Some(exit_code) = output.get("exit_code") {
        let mut line = format!("{name} exit_code={}", display_json_atom(exit_code));
        if let Some(stderr) = output.get("stderr").and_then(|v| v.as_str()) {
            if !stderr.trim().is_empty() {
                line.push_str(&format!(" stderr={}", compact_text(stderr, 160)));
            }
        }
        if let Some(stdout) = output.get("stdout").and_then(|v| v.as_str()) {
            if !stdout.trim().is_empty()
                && output
                    .get("stderr")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .trim()
                    .is_empty()
            {
                line.push_str(&format!(" stdout={}", compact_text(stdout, 160)));
            }
        }
        return Some(line);
    }
    if name == "RunCheck" {
        if let Some(command) = output.get("command").and_then(|v| v.as_str()) {
            return Some(format!("{name} passed: {}", compact_text(command, 120)));
        }
    }
    None
}

fn file_mutation_result_location_summary(
    name: &str,
    output: &serde_json::Value,
    cwd: Option<&Path>,
) -> Option<String> {
    if !matches!(
        name,
        "FileWrite" | "FileEdit" | "Mkdir" | "Move" | "Remove" | "NotebookEdit"
    ) {
        return None;
    }

    let mut parts = Vec::new();
    if let Some(obj) = output.as_object() {
        for key in ["path", "from", "to"] {
            if let Some(value) = obj.get(key).and_then(|v| v.as_str()) {
                parts.push(format!("{key}={value}"));
            }
        }
    }
    if let Some(cwd) = cwd {
        parts.push(format!("cwd={}", cwd.display()));
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

fn compact_tool_payload(output: &serde_json::Value) -> String {
    if let Some(error) = output.get("error").and_then(|v| v.as_str()) {
        return compact_text(error, 220);
    }
    if let Some(failures) = output.get("failures").and_then(|v| v.as_array()) {
        let joined = failures
            .iter()
            .filter_map(|v| v.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        if !joined.is_empty() {
            return compact_text(&joined, 220);
        }
    }
    compact_text(&output.to_string(), 220)
}

fn display_json_atom(value: &serde_json::Value) -> String {
    value
        .as_i64()
        .map(|n| n.to_string())
        .or_else(|| value.as_str().map(str::to_string))
        .unwrap_or_else(|| value.to_string())
}

fn compact_text(text: &str, max_chars: usize) -> String {
    let mut one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= max_chars {
        return one_line;
    }
    let keep = max_chars.saturating_sub(3);
    one_line = one_line.chars().take(keep).collect();
    one_line.push_str("...");
    one_line
}

async fn save_durable_session(
    engine: &ZodeEngine,
    meta: &DurableSessionMeta,
) -> Result<(), zode_core::CoreError> {
    let snapshot = engine
        .store
        .lock()
        .map_err(|_| zode_core::CoreError::Other("message store lock poisoned".into()))?
        .clone();
    SessionStore::open_default()?.save(meta, &snapshot).await
}

pub(crate) fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn tool_result_line_surfaces_failed_error() {
        let line = tool_result_line(
            Some("FileEdit"),
            false,
            &json!({"error": "old_string not found\nretry with FileRead"}),
            None,
        )
        .unwrap();
        assert!(line.contains("FileEdit failed"), "{line}");
        assert!(line.contains("old_string not found"), "{line}");
        assert!(line.contains("FileRead"), "{line}");
    }

    #[test]
    fn tool_result_line_surfaces_bash_exit_code_and_stderr() {
        let line = tool_result_line(
            Some("Bash"),
            true,
            &json!({"exit_code": 52, "stdout": "", "stderr": "Empty reply from server\n"}),
            None,
        )
        .unwrap();
        assert!(line.contains("Bash exit_code=52"), "{line}");
        assert!(line.contains("Empty reply from server"), "{line}");
    }

    #[test]
    fn tool_result_line_surfaces_failed_run_check() {
        let line = tool_result_line(
            Some("RunCheck"),
            true,
            &json!({"passed": false, "failures": ["expected stdout to contain ready"]}),
            None,
        )
        .unwrap();
        assert!(line.contains("RunCheck failed"), "{line}");
        assert!(line.contains("expected stdout"), "{line}");
    }

    #[test]
    fn tool_result_line_surfaces_file_result_location_and_cwd() {
        let cwd = std::path::Path::new("/work/project");
        let line = tool_result_line(
            Some("FileWrite"),
            true,
            &json!({"path": "/work/project/created.txt", "status": "ok", "size_bytes": 1}),
            Some(cwd),
        )
        .unwrap();
        assert!(line.contains("FileWrite done"), "{line}");
        assert!(line.contains("path=/work/project/created.txt"), "{line}");
        assert!(line.contains("cwd=/work/project"), "{line}");
    }
}
