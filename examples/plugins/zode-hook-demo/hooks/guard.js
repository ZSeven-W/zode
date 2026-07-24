// A Zode JavaScript hook. It runs in Zode's sandboxed QuickJS runtime — no
// filesystem, network, terminal, or process access — and is invoked in-process
// (no shell, no Node required) before every Bash tool call.
//
// Register a synchronous handler with zode.hook(fn). The handler receives the
// event and returns an outcome:
//   - { ok: true } / nothing / true        → allow (proceed)
//   - { block: true, reason } / "reason"    → block the tool call
//   - { warn: <code> }                      → log a warning, then proceed
//
// The event shape here is:
//   { event: "before_tool_use", tool: "Bash", input: { command: "..." } }

const DANGEROUS = [
  /\brm\s+-[a-z]*r[a-z]*f?\b.*\s(\/|~|\$HOME)/,       // rm -rf on an absolute or home path
  /\bgit\s+push\b.*--force(?!-with-lease)/,           // force push without a lease
  /\b(mkfs|fdisk)\b/,                                 // format / repartition
  /\bdd\b.*\bof=\/dev\//,                             // raw-disk overwrite
  />\s*\/dev\/(sd|nvme|disk)/,                        // redirect onto a raw disk
  /:\s*\(\s*\)\s*\{.*\}\s*;\s*:/,                     // fork bomb
];

zode.hook((event) => {
  if (event.tool !== "Bash") return { ok: true };

  const command = (event.input && event.input.command) || "";
  for (const pattern of DANGEROUS) {
    if (pattern.test(command)) {
      return {
        block: true,
        reason: `zode-hook-demo blocked a destructive command: ${command}`,
      };
    }
  }
  return { ok: true };
});
