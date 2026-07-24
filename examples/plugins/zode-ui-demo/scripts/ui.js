const percent = (ctx) =>
  ctx.context.usedPercent == null ? "?" : String(ctx.context.usedPercent);

const workspaceName = (ctx) => {
  const parts = String(ctx.session.cwd).split(/[\\/]/).filter(Boolean);
  return parts.at(-1) ?? String(ctx.session.cwd);
};

zode.data.define("githubRate", {
  refreshIntervalMs: 60000,
  request: {
    url: "https://api.github.com/rate_limit",
    timeoutMs: 3000,
    headers: {
      Accept: "application/vnd.github+json",
      "User-Agent": "zode-ui-demo"
    }
  }
});

const githubRemaining = (ctx) =>
  ctx.data.githubRate?.data?.resources?.core?.remaining ?? "…";

const toolActivity = (ctx) =>
  ctx.tools?.active?.[0] ?? ctx.tools?.recent?.at(-1)?.name ?? "idle";

zode.ui.sidebar((ctx) => ({
  lines: [
    {
      spans: [
        { text: "◆ UI plugin", tone: "accent", bold: true }
      ]
    },
    {
      spans: [
        { text: "model  ", tone: "muted" },
        { text: ctx.model.id, tone: "default" }
      ]
    },
    {
      spans: [
        { text: "ctx    ", tone: "muted" },
        {
          text: `${percent(ctx)}%`,
          tone: (ctx.context.usedPercent ?? 0) >= 75 ? "warning" : "success"
        }
      ]
    },
    {
      spans: [
        { text: "cwd    ", tone: "muted" },
        { text: workspaceName(ctx), tone: "default" }
      ]
    },
    {
      spans: [
        { text: "gh api ", tone: "muted" },
        { text: String(githubRemaining(ctx)), tone: "accent", bold: true }
      ]
    },
    {
      spans: [
        { text: "tool   ", tone: "muted" },
        { text: toolActivity(ctx), tone: "default" }
      ]
    }
  ]
}));

zode.ui.statusLine((ctx) => ({
  spans: [
    { text: "◆ zode-ui-demo", tone: "accent", bold: true },
    { text: `  ${ctx.session.title}`, tone: "default" },
    { text: `  ${ctx.status.mode}`, tone: ctx.session.busy ? "warning" : "success" },
    { text: `  ctx ${percent(ctx)}%`, tone: "muted" },
    { text: `  gh ${githubRemaining(ctx)}`, tone: "accent" },
    { text: `  tool ${toolActivity(ctx)}`, tone: "muted" }
  ]
}));
