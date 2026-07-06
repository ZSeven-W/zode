# OpenPencil Single Command Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `/op <design request>` the primary OpenPencil user command while keeping raw MCP access hidden behind compatibility paths.

**Architecture:** Keep the existing zode OpenPencil bridge: `commands/op.rs` parses slash-command text, `zode-tui/src/app.rs` dispatches `Generate` to the design pipeline and `Call` to raw MCP, and `openpencil/mod.rs` classifies `op_read` versus `op_write`. The implementation changes parser defaults and UI hints, not the design pipeline or OpenPencil lifecycle.

**Tech Stack:** Rust 2021, Cargo, serde_json, existing zode-core and zode-tui unit tests.

---

## File Structure

- Modify `crates/zode-core/src/commands/op.rs`: parse `/op <natural language>` as `Generate`, keep hidden compatibility subcommands, and update parser tests.
- Modify `crates/zode-core/src/openpencil/mod.rs`: expand the explicit read-tool classification to match the current OpenPencil MCP read surface.
- Modify `crates/zode-core/src/openpencil/tools.rs`: expand `read_classification_covers_real_reads` and mutating-tool regression tests.
- Modify `crates/zode-tui/src/ui/autocomplete_subhints.rs`: reduce `/op` hints to user-facing `status` plus hidden raw `call`; remove raw OpenPencil tool names and legacy `design` / `generate` prompts from the popup.
- Modify `crates/zode-tui/src/ui/autocomplete.rs`: update tests for the reduced `/op` hint list.
- Modify `CLAUDE.md`: document `/op <design request>` as the primary command and raw `/op call` as a hidden diagnostic path.

## Task 1: Parser TDD

**Files:**
- Modify: `crates/zode-core/src/commands/op.rs`

- [ ] **Step 1: Replace parser tests with the new public default behavior**

In `crates/zode-core/src/commands/op.rs`, update the `#[cfg(test)] mod tests` block so it contains these focused tests:

```rust
#[test]
fn maps_status_design_call_compatibility_paths() {
    assert!(matches!(
        map_subcommand("status").unwrap(),
        OpCommand::Status
    ));
    match map_subcommand("design F1=I(\"p\",{})").unwrap() {
        OpCommand::Call { tool, args } => {
            assert_eq!(tool, "batch_design");
            assert_eq!(args["operations"], "F1=I(\"p\",{})");
            assert!(args.get("dsl").is_none());
        }
        _ => panic!(),
    }
    match map_subcommand("call insert_node {\"x\":1}").unwrap() {
        OpCommand::Call { tool, args } => {
            assert_eq!(tool, "insert_node");
            assert_eq!(args["x"], 1);
        }
        _ => panic!(),
    }
}

#[test]
fn natural_language_maps_to_generate() {
    for input in [
        "a pricing dashboard",
        "get_document_info",
        "做一个移动端首页",
    ] {
        match map_subcommand(input).unwrap() {
            OpCommand::Generate { prompt } => assert_eq!(prompt, input),
            other => panic!("expected Generate for {input:?}, got {other:?}"),
        }
    }
}

#[test]
fn maps_generate_alias() {
    match map_subcommand("generate a pricing page").unwrap() {
        OpCommand::Generate { prompt } => assert_eq!(prompt, "a pricing page"),
        _ => panic!(),
    }
    assert!(map_subcommand("generate").is_err());
}

#[test]
fn empty_errs() {
    assert!(map_subcommand("").is_err());
}
```

- [ ] **Step 2: Run parser tests and confirm the expected failures**

Run:

```bash
cargo test -p zode-core commands::op
```

Expected before implementation: failures showing `design` still writes `dsl` and `get_document_info` still maps to `Call`.

- [ ] **Step 3: Implement parser behavior**

Update `map_subcommand`:

```rust
pub fn map_subcommand(args: &str) -> Result<OpCommand, String> {
    let args = args.trim();
    let (head, rest) = match args.split_once(char::is_whitespace) {
        Some((h, r)) => (h, r.trim()),
        None => (args, ""),
    };
    match head {
        "" => Err("usage: /op <design request>".into()),
        "status" => Ok(OpCommand::Status),
        "design" => {
            if rest.is_empty() {
                return Err("usage: /op design '<operations>'".into());
            }
            Ok(OpCommand::Call {
                tool: "batch_design".into(),
                args: json!({ "operations": rest }),
            })
        }
        "call" => {
            let (tool, payload) = rest.split_once(char::is_whitespace).unwrap_or((rest, "{}"));
            if tool.is_empty() {
                return Err("usage: /op call <tool> <json>".into());
            }
            let v: Value = serde_json::from_str(payload.trim()).map_err(|e| e.to_string())?;
            Ok(OpCommand::Call {
                tool: tool.into(),
                args: v,
            })
        }
        "generate" => {
            if rest.is_empty() {
                return Err("usage: /op generate <prompt>".into());
            }
            Ok(OpCommand::Generate {
                prompt: rest.to_string(),
            })
        }
        _ => Ok(OpCommand::Generate {
            prompt: args.to_string(),
        }),
    }
}
```

Also update the module-level comment to say `/op <design request>` is the public default and `call`/`design` are compatibility paths.

- [ ] **Step 4: Verify parser tests pass**

Run:

```bash
cargo test -p zode-core commands::op
```

Expected: all `commands::op` tests pass.

- [ ] **Step 5: Commit parser change**

Run:

```bash
git add crates/zode-core/src/commands/op.rs
git commit -m "feat(op): route natural language prompts to design generation"
```

## Task 2: OpenPencil Read/Write Classification

**Files:**
- Modify: `crates/zode-core/src/openpencil/mod.rs`
- Modify: `crates/zode-core/src/openpencil/tools.rs`

- [ ] **Step 1: Expand classification tests first**

In `crates/zode-core/src/openpencil/tools.rs`, replace the read-classification test body with:

```rust
#[test]
fn read_classification_covers_real_reads() {
    for t in [
        "open_document",
        "get_document_info",
        "get_selection",
        "get_node",
        "get_node_children",
        "get_node_parent",
        "list_pages",
        "list_variables",
        "get_variables",
        "conversion_status",
        "lint_document",
        "list_theme_presets",
        "get_design_md",
        "export_design_md",
        "get_style_guide_tags",
        "get_style_guide",
        "get_guidelines",
        "ToolSearch",
        "get_screenshot",
        "get_active_theme",
        "list_components",
        "get_component",
        "snapshot_layout",
        "find_empty_space",
        "get_canvas_bounds",
        "find_node_by_name",
        "count_nodes",
        "list_node_kinds",
        "get_history_depth",
        "get_viewport",
        "get_selection_set",
        "get_editor_state",
        "read_nodes",
        "batch_get",
        "search_all_unique_properties",
    ] {
        assert!(is_read_tool(t), "{t} should be read");
    }
    for t in [
        "save_document",
        "upsert_variables",
        "upsert_component",
        "upsert_screen",
        "save_theme_preset",
        "load_theme_preset",
        "set_design_md",
        "spawn_agents",
        "export_nodes",
        "codegen_plan",
        "codegen_submit_chunk",
        "codegen_assemble",
        "codegen_clean",
        "replace_all_matching_properties",
        "batch_design",
        "design_skeleton",
        "design_content",
        "design_refine",
        "insert_node",
        "delete_node",
        "set_node_fill_hex",
    ] {
        assert!(!is_read_tool(t), "{t} should be write");
    }
}
```

- [ ] **Step 2: Run classification test and confirm failures**

Run:

```bash
cargo test -p zode-core openpencil::tools::tests::read_classification_covers_real_reads
```

Expected before implementation: failures for at least `open_document`, `ToolSearch`, and `find_empty_space`.

- [ ] **Step 3: Expand the explicit read set**

In `crates/zode-core/src/openpencil/mod.rs`, update the `READ_TOOLS` constant inside `is_read_tool` to include the current read set:

```rust
const READ_TOOLS: &[&str] = &[
    "open_document",
    "get_document_info",
    "get_selection",
    "get_node",
    "get_node_children",
    "get_node_parent",
    "list_pages",
    "list_variables",
    "get_variables",
    "conversion_status",
    "lint_document",
    "list_theme_presets",
    "get_design_md",
    "export_design_md",
    "get_style_guide_tags",
    "get_style_guide",
    "get_guidelines",
    "ToolSearch",
    "get_screenshot",
    "get_active_theme",
    "list_components",
    "get_component",
    "snapshot_layout",
    "find_empty_space",
    "get_canvas_bounds",
    "find_node_by_name",
    "count_nodes",
    "list_node_kinds",
    "get_history_depth",
    "get_viewport",
    "get_selection_set",
    "get_editor_state",
    "read_nodes",
    "batch_get",
    "export_design_md",
    "search_all_unique_properties",
];
```

Keep the existing prefix heuristic after the explicit set.

- [ ] **Step 4: Verify classification test passes**

Run:

```bash
cargo test -p zode-core openpencil::tools::tests::read_classification_covers_real_reads
```

Expected: the classification test passes.

- [ ] **Step 5: Commit classification change**

Run:

```bash
git add crates/zode-core/src/openpencil/mod.rs crates/zode-core/src/openpencil/tools.rs
git commit -m "fix(op): sync OpenPencil read tool classification"
```

## Task 3: Autocomplete Hints

**Files:**
- Modify: `crates/zode-tui/src/ui/autocomplete_subhints.rs`
- Modify: `crates/zode-tui/src/ui/autocomplete.rs`

- [ ] **Step 1: Update autocomplete tests first**

In `crates/zode-tui/src/ui/autocomplete.rs`, change the `/op` subcommand tests:

```rust
#[test]
fn op_sub_filters_by_typed_prefix() {
    let mut ac = Autocomplete::new();
    ac.update("/op sta");
    assert!(ac.is_op_sub_active());
    let confirmed = ac.op_sub_confirm().expect("should match 'status'");
    assert_eq!(confirmed, "/op status");
}

#[test]
fn op_sub_confirm_appends_space_for_call_only() {
    let mut ac = Autocomplete::new();
    ac.update("/op call");
    assert!(ac.is_op_sub_active());
    let text = ac.op_sub_confirm().expect("call match");
    assert_eq!(text, "/op call ");
}

#[test]
fn op_sub_renders_user_facing_entries() {
    let theme = crate::theme::ThemeStore::with_builtins().resolve(Some("cyberpunk"));
    let backend = ratatui::backend::TestBackend::new(110, 24);
    let mut term = ratatui::Terminal::new(backend).unwrap();
    let input_area = Rect::new(0, 20, 110, 4);
    let mut ac = Autocomplete::new();

    ac.update("/op ");
    term.draw(|f| ac.render(f, input_area, &theme)).unwrap();

    let content: String = term
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(content.contains("status"), "popup should show 'status'");
    assert!(content.contains("call"), "popup should show hidden raw-call escape hatch");
    assert!(!content.contains("design"), "popup should not advertise legacy direct DSL");
    assert!(!content.contains("generate"), "popup should not advertise prompt alias");
    assert!(!content.contains("get_document_info"), "popup should hide raw MCP tools");
}

#[test]
fn op_subcommands_constant_covers_public_entries() {
    let expected = ["status", "call"];
    assert_eq!(OP_SUBCOMMANDS, expected);
    assert_eq!(OP_SUBCOMMANDS.len(), OP_SUBCOMMAND_DESCS.len());
}
```

Remove the old `op_sub_confirm_appends_space_for_design_and_call` expectations for `design`.

- [ ] **Step 2: Run autocomplete tests and confirm failures**

Run:

```bash
cargo test -p zode-tui op_sub
```

Expected before implementation: tests fail because `design`, `generate`, and read tools are still in `OP_SUBCOMMANDS`.

- [ ] **Step 3: Reduce `/op` hint table**

In `crates/zode-tui/src/ui/autocomplete_subhints.rs`, replace the `/op` constants and comments:

```rust
/// `/op` hint entries. `/op <free text>` is the primary design-generation
/// command, so the popup only advertises diagnostics plus the raw-call escape
/// hatch. Legacy `design` / `generate` compatibility paths remain parser-only.
pub const OP_SUBCOMMANDS: &[&str] = &["status", "call"];

/// Brief descriptions shown alongside each `/op` entry in the hint popup.
pub(crate) const OP_SUBCOMMAND_DESCS: &[&str] = &[
    "report connection state",
    "call an MCP tool by name",
];

/// `/op` entries that take a required argument, so `SubHints::confirm` should
/// leave a trailing space instead of submitting bare.
pub(crate) const OP_SUB_TRAILING_SPACE: &[&str] = &["call"];
```

- [ ] **Step 4: Verify autocomplete tests pass**

Run:

```bash
cargo test -p zode-tui op_sub
```

Expected: all `/op` autocomplete tests pass.

- [ ] **Step 5: Commit autocomplete change**

Run:

```bash
git add crates/zode-tui/src/ui/autocomplete_subhints.rs crates/zode-tui/src/ui/autocomplete.rs
git commit -m "fix(tui): simplify OpenPencil command hints"
```

## Task 4: Documentation

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update the OpenPencil control command documentation**

In `CLAUDE.md`, replace the `/op` slash command section with text that says:

```markdown
### `/op` slash command

Type `/op <design request>` in the TUI input. This is the primary user-facing
OpenPencil flow: zode connects to a running OpenPencil instance, launches it if
needed after consent, then runs the design pipeline (plan → skeleton → content
→ refine). Users do not need to know OpenPencil MCP tool names for normal
design generation.

Compatibility / diagnostic forms:

| Command | Effect |
|---------|--------|
| `/op <design request>` | Run the design pipeline from natural language |
| `/op status` | Print connection state (connected / port / none) |
| `/op call <tool> <json>` | Hidden escape hatch for explicit MCP tool calls |
| `/op design '<operations>'` | Hidden compatibility path for `batch_design` `operations` |
| `/op generate <prompt>` | Hidden alias for `/op <prompt>` |

Autocomplete hints recommend only `status` and `call`. Raw OpenPencil MCP tool
names are intentionally not advertised in the user-facing command flow.
```

Also update the `op_read` paragraph to mention the expanded read set: include
`open_document`, `get_editor_state`, `get_variables`, `get_guidelines`,
`get_style_guide`, `get_screenshot`, `ToolSearch`, `find_empty_space`,
`read_nodes`, `batch_get`, `export_design_md`, and
`search_all_unique_properties`.

Change the heading `Design generation (op_design / /op generate)` to
`Design generation (op_design / /op <prompt>)`, and update the paragraph that
describes `/op generate <prompt>` so it says `/op <prompt>` maps to
`OpCommand::Generate`; `/op generate <prompt>` remains a hidden compatibility
alias.

- [ ] **Step 2: Review doc diff**

Run:

```bash
git diff -- CLAUDE.md
```

Expected: only the OpenPencil control section changes.

- [ ] **Step 3: Commit documentation**

Run:

```bash
git add CLAUDE.md
git commit -m "docs(op): document single command design flow"
```

## Task 5: Final Verification

**Files:**
- Verify all modified files.

- [ ] **Step 1: Run focused core tests**

Run:

```bash
cargo test -p zode-core op
```

Expected: all filtered zode-core tests pass.

- [ ] **Step 2: Run focused TUI autocomplete tests**

Run:

```bash
cargo test -p zode-tui op_sub
```

Expected: all `/op` autocomplete tests pass.

- [ ] **Step 3: Run formatting check**

Run:

```bash
cargo fmt --all -- --check
```

Expected: no formatting diff.

- [ ] **Step 4: Inspect final diff**

Run:

```bash
git diff --stat HEAD~4..HEAD
git status --short
```

Expected: commits include the spec, parser, classification, autocomplete, and docs changes; worktree is clean.

- [ ] **Step 5: If needed, fix formatting**

If `cargo fmt --all -- --check` reports changed Rust files, run:

```bash
cargo fmt --all
git status --short
git add crates/zode-core/src/commands/op.rs crates/zode-core/src/openpencil/mod.rs crates/zode-core/src/openpencil/tools.rs crates/zode-tui/src/ui/autocomplete_subhints.rs crates/zode-tui/src/ui/autocomplete.rs
git commit -m "style: format OpenPencil command changes"
```

Expected after this step: `cargo fmt --all -- --check` passes.
