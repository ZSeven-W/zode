# Zode OpenPencil Single Command Design

## Context

Zode integrates with a running OpenPencil instance through the local OpenPencil
MCP endpoint. The current bridge exposes several implementation details to the
user:

- `/op generate <prompt>` for the design pipeline.
- `/op design <dsl>` for direct `batch_design` calls.
- `/op call <tool> <json>` and arbitrary `/op <tool> <json>` passthroughs.
- Agent-facing `op_read`, `op_write`, and `op_design` tools.

OpenPencil has also expanded its Rust MCP contract. The current `batch_design`
contract uses `operations`, `script`, or `nodes_json`; the old zode
`/op design` mapping still sends `dsl`. OpenPencil now exposes additional read
tools such as `open_document`, `get_editor_state`, `get_variables`,
`get_guidelines`, `get_style_guide`, `get_screenshot`, and `ToolSearch`.

The requested product direction is to keep the user-facing command simple:
users should type one command and describe the design. Zode should handle the
OpenPencil lifecycle and tool orchestration internally.

## Goals

- Make `/op <design request>` the primary user-facing OpenPencil command.
- Hide OpenPencil MCP tool names from normal UI/help/autocomplete flows.
- Keep existing internal bridge architecture: lifecycle connection, design
  pipeline, and agent tools remain separate implementation layers.
- Sync zode's bridge contract with the current local OpenPencil MCP surface.
- Avoid changing the OpenPencil release tag or editing the OpenPencil repo.

## Non-Goals

- Do not add a second public command such as `/design`.
- Do not dynamically infer read/write safety from `tools/list`; OpenPencil does
  not yet expose first-class safety metadata.
- Do not remove internal `op_read`, `op_write`, or `op_design`; they remain
  agent-facing implementation tools.
- Do not expose raw MCP tool names in the normal user workflow.

## User-Facing Behavior

The primary command is:

```text
/op <design request>
```

Examples:

```text
/op design a mobile personal finance home screen
/op 做一个深色风格的 SaaS billing dashboard
```

The command starts the existing OpenPencil design generation pipeline:

1. Discover a running OpenPencil MCP endpoint.
2. If needed, ask for consent and launch/install through the existing bridge.
3. Ask the current model for a design plan.
4. Call `design_skeleton`.
5. Call `design_content` for each section.
6. Call `design_refine`.
7. Report progress and final success/failure summary in the zode transcript.

Bare `/op` remains an error with a short usage message.

## Compatibility Paths

Some old subcommands remain available as hidden compatibility paths:

- `/op status` returns the local connection status.
- `/op generate <prompt>` is an alias for `/op <prompt>`.
- `/op design <operations>` calls `batch_design` with
  `{ "operations": "<operations>" }`.
- `/op call <tool> <json>` calls an explicit OpenPencil MCP tool.

The old arbitrary passthrough form `/op <tool> <json>` is not part of the
primary behavior because it conflicts with natural-language design prompts.
Callers that need raw tools should use `/op call`.

Autocomplete and help should recommend only `/op <design request>` and, if
needed, `/op status` as a diagnostic. They should not advertise raw tool names.

## Parser Design

`commands/op.rs` keeps the existing `OpCommand` enum:

- `Status`
- `Generate { prompt }`
- `Call { tool, args }`

Parsing rules:

- Empty input returns usage.
- Exact `status` maps to `Status`.
- `generate <prompt>` maps to `Generate`.
- `design <operations>` maps to
  `Call { tool: "batch_design", args: { "operations": operations } }`.
- `call <tool> <json>` maps to `Call`.
- Everything else maps to `Generate { prompt: full_input }`.

This makes non-English prompts and prompts that start with arbitrary words work
without being mistaken for MCP tool names.

## Tool Classification Sync

`is_read_tool` remains a static zode-side classification until OpenPencil
exposes formal safety metadata.

Add current OpenPencil read tools to the explicit read set, including:

- `open_document`
- `get_document_info`
- `get_selection`
- `get_node`
- `get_node_children`
- `get_node_parent`
- `list_pages`
- `list_variables`
- `get_variables`
- `conversion_status`
- `lint_document`
- `list_theme_presets`
- `get_design_md`
- `export_design_md`
- `get_style_guide_tags`
- `get_style_guide`
- `get_guidelines`
- `ToolSearch`
- `get_screenshot`
- `get_active_theme`
- `list_components`
- `get_component`
- `batch_get`
- `read_nodes`
- `search_all_unique_properties`
- `snapshot_layout`
- `find_empty_space`
- `get_canvas_bounds`
- `find_node_by_name`
- `count_nodes`
- `list_node_kinds`
- `get_history_depth`
- `get_viewport`
- `get_selection_set`
- `get_editor_state`

Continue routing mutating or create-like tools through `op_write`, including:

- `save_document`
- `upsert_variables`
- `upsert_component`
- `upsert_screen`
- `save_theme_preset`
- `load_theme_preset`
- `set_design_md`
- `spawn_agents`
- `export_nodes`
- `codegen_plan`
- `codegen_submit_chunk`
- `codegen_assemble`
- `codegen_clean`
- `replace_all_matching_properties`
- `batch_design`
- `design_skeleton`
- `design_content`
- `design_refine`
- all `set_*`, `insert_*`, `update_*`, `delete_*`, `move_*`, `copy_*`,
  `clear_*`, `toggle_*`, `duplicate_*`, `group_*`, `ungroup_*`, `undo`, and
  `redo` tools.

## Documentation

Update `zode/CLAUDE.md` to describe:

- `/op <design request>` as the primary command.
- Raw MCP calls as hidden/diagnostic compatibility via `/op call`.
- `/op design` using the `operations` field, not `dsl`.
- Expanded read-tool classification.

## Tests

Update or add focused tests:

- `map_subcommand("a pricing dashboard")` returns `Generate`.
- `map_subcommand("做一个移动端首页")` returns `Generate`.
- `map_subcommand("generate a pricing page")` remains `Generate`.
- `map_subcommand("design root=I(null,{type:'frame'})")` returns
  `batch_design` with `operations`.
- `map_subcommand("call get_document_info {}")` remains an explicit raw call.
- `map_subcommand("get_document_info")` returns `Generate`, proving arbitrary
  passthrough is no longer the default.
- `is_read_tool` accepts the current OpenPencil read set.
- `is_read_tool` rejects representative mutating/create tools such as
  `batch_design`, `export_nodes`, `spawn_agents`, `codegen_plan`, and
  `set_design_md`.

## Risk

The main behavior change is that `/op <tool>` is no longer an arbitrary
passthrough. This is intentional to make `/op <natural language>` reliable.
Raw tool access remains available through `/op call`.
