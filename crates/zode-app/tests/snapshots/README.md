# Platform screenshot goldens

Each supported desktop OS owns its rendered PNG baselines in a separate
directory:

- `macos/`
- `windows/`
- `linux/`

Generate only on the matching OS:

```bash
ZODE_UPDATE_SNAPSHOTS=1 cargo +1.94 test -p zode-app --test snapshots
cargo +1.94 test -p zode-app --test snapshots
```

Inspect every generated image before committing it. Confirm the expected
layout and Zode branding, readable Chinese text, and the absence of Codex
logos or assets. For a non-local platform, use the manual `update-snapshots`
CI job for that OS and review its uploaded artifact before adding the PNGs.

The suite also fixes the key `WorkspaceLayout` rectangles for every scene.
Their maximum drift is measured in physical pixels and must stay at or below
2 px, independently of the broader pixel-difference budget. Text paint and
measurement use the repository-private `Zode Snapshot Sans SC` family from
`../fonts/` on every platform.
