//! The `lsp_*` tools — the agent-facing surface of the LSP plugin. Each is a
//! thin `Tool` impl that resolves the target file to a language server (via the
//! [`LspManager`]), opens it, issues one LSP request, and returns a compact,
//! model-friendly result. All are read-only: `lsp_rename` / `lsp_format` return
//! the server's proposed edits as a *preview* rather than writing to disk, so
//! the agent stays in control of mutations (apply them with FileEdit).

use std::sync::Arc;
use std::time::Duration;

use agent::error::AgentError;
use agent::tool::{SafetyClass, Tool, ToolUseContext};
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::lsp::client::LspClient;
use crate::lsp::manager::LspManager;

/// How long `lsp_diagnostics` waits for the server's first publish.
const DIAG_WAIT_SECS: u64 = 8;

/// Build all seven tools over one manager.
pub fn all_tools(mgr: &Arc<LspManager>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(LspDiagnosticsTool(mgr.clone())),
        Arc::new(LspHoverTool(mgr.clone())),
        Arc::new(LspDefinitionTool(mgr.clone())),
        Arc::new(LspReferencesTool(mgr.clone())),
        Arc::new(LspSymbolsTool(mgr.clone())),
        Arc::new(LspRenameTool(mgr.clone())),
        Arc::new(LspFormatTool(mgr.clone())),
    ]
}

// ---- shared helpers --------------------------------------------------------

fn file_arg(input: &Value) -> Result<String, AgentError> {
    input
        .get("file")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .ok_or_else(|| AgentError::other("missing required string 'file'"))
}

/// LSP positions are 0-based (line, character).
fn position(input: &Value) -> Result<Value, AgentError> {
    let line = input
        .get("line")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| AgentError::other("missing required integer 'line' (0-based)"))?;
    let character = input.get("character").and_then(|v| v.as_u64()).unwrap_or(0);
    Ok(json!({ "line": line, "character": character }))
}

/// Resolve `file`, open it, and hand back (client, uri).
async fn open(
    mgr: &Arc<LspManager>,
    file: &str,
) -> Result<(Arc<LspClient>, String), AgentError> {
    let path = mgr.resolve(file);
    let client = mgr.client_for(&path).await.map_err(AgentError::other)?;
    let uri = client.ensure_open(&path).await.map_err(AgentError::other)?;
    Ok((client, uri))
}

fn file_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "file": { "type": "string", "description": "Path to the source file (absolute or workspace-relative)." } },
        "required": ["file"]
    })
}

fn position_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "file": { "type": "string", "description": "Path to the source file." },
            "line": { "type": "integer", "description": "0-based line number." },
            "character": { "type": "integer", "description": "0-based column (default 0)." }
        },
        "required": ["file", "line"]
    })
}

const SEVERITY: [&str; 5] = ["", "error", "warning", "info", "hint"];

fn simplify_diagnostic(d: &Value) -> Value {
    let start = d.pointer("/range/start");
    let sev = d
        .get("severity")
        .and_then(|v| v.as_u64())
        .and_then(|n| SEVERITY.get(n as usize))
        .copied()
        .unwrap_or("");
    json!({
        "line": start.and_then(|s| s.get("line")).and_then(|v| v.as_u64()),
        "character": start.and_then(|s| s.get("character")).and_then(|v| v.as_u64()),
        "severity": sev,
        "message": d.get("message").and_then(|v| v.as_str()).unwrap_or(""),
        "source": d.get("source").and_then(|v| v.as_str()),
    })
}

/// Normalize a Location | Location[] | LocationLink[] into `[{file,line,character}]`.
fn normalize_locations(result: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    let mut push = |loc: &Value| {
        // LocationLink uses `targetUri`/`targetRange`; Location uses `uri`/`range`.
        let uri = loc
            .get("uri")
            .or_else(|| loc.get("targetUri"))
            .and_then(|v| v.as_str());
        let range = loc.get("range").or_else(|| loc.get("targetRange"));
        if let (Some(uri), Some(range)) = (uri, range) {
            let start = range.get("start");
            out.push(json!({
                "file": LspClient::uri_to_display(uri),
                "line": start.and_then(|s| s.get("line")).and_then(|v| v.as_u64()),
                "character": start.and_then(|s| s.get("character")).and_then(|v| v.as_u64()),
            }));
        }
    };
    match result {
        Value::Array(items) => items.iter().for_each(&mut push),
        Value::Object(_) => push(result),
        _ => {}
    }
    out
}

fn hover_text(result: &Value) -> Option<String> {
    let contents = result.get("contents")?;
    match contents {
        // MarkupContent { kind, value }
        Value::Object(o) => o
            .get("value")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            // MarkedString { language, value }
            .or_else(|| o.get("value").and_then(|v| v.as_str()).map(str::to_string)),
        Value::String(s) => Some(s.clone()),
        Value::Array(items) => {
            let parts: Vec<String> = items
                .iter()
                .filter_map(|i| match i {
                    Value::String(s) => Some(s.clone()),
                    Value::Object(o) => o.get("value").and_then(|v| v.as_str()).map(str::to_string),
                    _ => None,
                })
                .collect();
            (!parts.is_empty()).then(|| parts.join("\n"))
        }
        _ => None,
    }
}

// ---- the tools -------------------------------------------------------------

macro_rules! readonly {
    () => {
        fn safety_class(&self) -> SafetyClass {
            SafetyClass::ReadOnly
        }
    };
}

#[derive(Debug)]
struct LspDiagnosticsTool(Arc<LspManager>);
#[async_trait]
impl Tool for LspDiagnosticsTool {
    fn name(&self) -> &str {
        "lsp_diagnostics"
    }
    fn description(&self) -> &str {
        "Compiler/linter diagnostics (errors, warnings) for a file, from its language server."
    }
    fn input_schema(&self) -> Value {
        file_schema()
    }
    readonly!();
    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let file = file_arg(&input)?;
        let (client, uri) = open(&self.0, &file).await?;
        let diags = client
            .diagnostics_for(&uri, Duration::from_secs(DIAG_WAIT_SECS))
            .await;
        let simplified: Vec<Value> = diags.iter().map(simplify_diagnostic).collect();
        Ok(json!({ "file": file, "count": simplified.len(), "diagnostics": simplified }))
    }
}

#[derive(Debug)]
struct LspHoverTool(Arc<LspManager>);
#[async_trait]
impl Tool for LspHoverTool {
    fn name(&self) -> &str {
        "lsp_hover"
    }
    fn description(&self) -> &str {
        "Type signature and docs for the symbol at a position (line/character, 0-based)."
    }
    fn input_schema(&self) -> Value {
        position_schema()
    }
    readonly!();
    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let file = file_arg(&input)?;
        let pos = position(&input)?;
        let (client, uri) = open(&self.0, &file).await?;
        let result = client
            .request(
                "textDocument/hover",
                json!({ "textDocument": { "uri": uri }, "position": pos }),
            )
            .await
            .map_err(AgentError::other)?;
        Ok(json!({ "hover": hover_text(&result) }))
    }
}

#[derive(Debug)]
struct LspDefinitionTool(Arc<LspManager>);
#[async_trait]
impl Tool for LspDefinitionTool {
    fn name(&self) -> &str {
        "lsp_definition"
    }
    fn description(&self) -> &str {
        "Jump to where the symbol at a position is defined. Returns file/line locations."
    }
    fn input_schema(&self) -> Value {
        position_schema()
    }
    readonly!();
    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let file = file_arg(&input)?;
        let pos = position(&input)?;
        let (client, uri) = open(&self.0, &file).await?;
        let result = client
            .request(
                "textDocument/definition",
                json!({ "textDocument": { "uri": uri }, "position": pos }),
            )
            .await
            .map_err(AgentError::other)?;
        Ok(json!({ "locations": normalize_locations(&result) }))
    }
}

#[derive(Debug)]
struct LspReferencesTool(Arc<LspManager>);
#[async_trait]
impl Tool for LspReferencesTool {
    fn name(&self) -> &str {
        "lsp_references"
    }
    fn description(&self) -> &str {
        "Find all references to the symbol at a position across the workspace."
    }
    fn input_schema(&self) -> Value {
        position_schema()
    }
    readonly!();
    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let file = file_arg(&input)?;
        let pos = position(&input)?;
        let (client, uri) = open(&self.0, &file).await?;
        let result = client
            .request(
                "textDocument/references",
                json!({
                    "textDocument": { "uri": uri }, "position": pos,
                    "context": { "includeDeclaration": true }
                }),
            )
            .await
            .map_err(AgentError::other)?;
        let locs = normalize_locations(&result);
        Ok(json!({ "count": locs.len(), "locations": locs }))
    }
}

#[derive(Debug)]
struct LspSymbolsTool(Arc<LspManager>);
#[async_trait]
impl Tool for LspSymbolsTool {
    fn name(&self) -> &str {
        "lsp_symbols"
    }
    fn description(&self) -> &str {
        "List the symbols (functions, types, fields) declared in a file."
    }
    fn input_schema(&self) -> Value {
        file_schema()
    }
    readonly!();
    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let file = file_arg(&input)?;
        let (client, uri) = open(&self.0, &file).await?;
        let result = client
            .request(
                "textDocument/documentSymbol",
                json!({ "textDocument": { "uri": uri } }),
            )
            .await
            .map_err(AgentError::other)?;
        let symbols = flatten_symbols(&result);
        Ok(json!({ "count": symbols.len(), "symbols": symbols }))
    }
}

#[derive(Debug)]
struct LspRenameTool(Arc<LspManager>);
#[async_trait]
impl Tool for LspRenameTool {
    fn name(&self) -> &str {
        "lsp_rename"
    }
    fn description(&self) -> &str {
        "Preview a workspace-wide rename of the symbol at a position. Returns proposed edits (does not write files)."
    }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file": { "type": "string" },
                "line": { "type": "integer", "description": "0-based line." },
                "character": { "type": "integer", "description": "0-based column." },
                "new_name": { "type": "string", "description": "Replacement identifier." }
            },
            "required": ["file", "line", "new_name"]
        })
    }
    readonly!();
    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let file = file_arg(&input)?;
        let pos = position(&input)?;
        let new_name = input
            .get("new_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AgentError::other("missing required string 'new_name'"))?;
        let (client, uri) = open(&self.0, &file).await?;
        let result = client
            .request(
                "textDocument/rename",
                json!({ "textDocument": { "uri": uri }, "position": pos, "newName": new_name }),
            )
            .await
            .map_err(AgentError::other)?;
        Ok(json!({ "preview": true, "edit": result }))
    }
}

#[derive(Debug)]
struct LspFormatTool(Arc<LspManager>);
#[async_trait]
impl Tool for LspFormatTool {
    fn name(&self) -> &str {
        "lsp_format"
    }
    fn description(&self) -> &str {
        "Get the language server's formatting edits for a file (preview; does not write the file)."
    }
    fn input_schema(&self) -> Value {
        file_schema()
    }
    readonly!();
    async fn call(&self, _ctx: &ToolUseContext, input: Value) -> Result<Value, AgentError> {
        let file = file_arg(&input)?;
        let (client, uri) = open(&self.0, &file).await?;
        let result = client
            .request(
                "textDocument/formatting",
                json!({
                    "textDocument": { "uri": uri },
                    "options": { "tabSize": 4, "insertSpaces": true }
                }),
            )
            .await
            .map_err(AgentError::other)?;
        let count = result.as_array().map(|a| a.len()).unwrap_or(0);
        Ok(json!({ "preview": true, "edits": result, "count": count }))
    }
}

const SYMBOL_KIND: [&str; 27] = [
    "", "file", "module", "namespace", "package", "class", "method", "property", "field",
    "constructor", "enum", "interface", "function", "variable", "constant", "string", "number",
    "boolean", "array", "object", "key", "null", "enum-member", "struct", "event", "operator",
    "type-parameter",
];

/// Flatten DocumentSymbol[] (nested) or SymbolInformation[] (flat) into a list
/// of `{name, kind, line}`.
fn flatten_symbols(result: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    fn kind_name(n: u64) -> &'static str {
        SYMBOL_KIND.get(n as usize).copied().unwrap_or("")
    }
    fn walk(node: &Value, out: &mut Vec<Value>) {
        let name = node.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let kind = node.get("kind").and_then(|v| v.as_u64()).unwrap_or(0);
        // DocumentSymbol: range; SymbolInformation: location.range
        let line = node
            .pointer("/range/start/line")
            .or_else(|| node.pointer("/location/range/start/line"))
            .and_then(|v| v.as_u64());
        out.push(json!({ "name": name, "kind": kind_name(kind), "line": line }));
        if let Some(children) = node.get("children").and_then(|v| v.as_array()) {
            for c in children {
                walk(c, out);
            }
        }
    }
    if let Some(items) = result.as_array() {
        for item in items {
            walk(item, &mut out);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_single_location_and_array() {
        let single = json!({ "uri": "file:///x.rs", "range": { "start": { "line": 3, "character": 2 } } });
        let locs = normalize_locations(&single);
        assert_eq!(locs.len(), 1);
        assert_eq!(locs[0]["file"], "/x.rs");
        assert_eq!(locs[0]["line"], 3);

        let link = json!([{ "targetUri": "file:///y.rs", "targetRange": { "start": { "line": 9, "character": 0 } } }]);
        let locs = normalize_locations(&link);
        assert_eq!(locs[0]["file"], "/y.rs");
        assert_eq!(locs[0]["line"], 9);
    }

    #[test]
    fn hover_extracts_markup_and_array() {
        let markup = json!({ "contents": { "kind": "markdown", "value": "fn main()" } });
        assert_eq!(hover_text(&markup).as_deref(), Some("fn main()"));
        let arr = json!({ "contents": ["a", { "value": "b" }] });
        assert_eq!(hover_text(&arr).as_deref(), Some("a\nb"));
        assert_eq!(hover_text(&json!({})), None);
    }

    #[test]
    fn simplifies_diagnostic_severity() {
        let d = json!({
            "range": { "start": { "line": 10, "character": 4 } },
            "severity": 1, "message": "mismatched types", "source": "rustc"
        });
        let s = simplify_diagnostic(&d);
        assert_eq!(s["severity"], "error");
        assert_eq!(s["line"], 10);
        assert_eq!(s["message"], "mismatched types");
        assert_eq!(s["source"], "rustc");
    }

    #[test]
    fn flattens_nested_document_symbols() {
        let result = json!([
            { "name": "Foo", "kind": 23, "range": { "start": { "line": 0, "character": 0 } },
              "children": [ { "name": "bar", "kind": 6, "range": { "start": { "line": 1, "character": 4 } } } ] }
        ]);
        let syms = flatten_symbols(&result);
        assert_eq!(syms.len(), 2);
        assert_eq!(syms[0]["name"], "Foo");
        assert_eq!(syms[0]["kind"], "struct");
        assert_eq!(syms[1]["name"], "bar");
        assert_eq!(syms[1]["kind"], "method");
    }
}
