# Zode 内置 Chrome 控制 — 设计

日期：2026-07-03
状态：已与用户逐节确认；已按 Codex 审查（10 条发现全部核实成立）修订；M1/M2 已实施（M2 见 `docs/superpowers/plans/2026-07-05-zode-browser-control-m2.md`）

## 背景与目标

zode 目前没有浏览器控制能力。目标是内置一套 Chrome 控制子系统，覆盖四个场景：

1. **前端开发验证** — agent 改完前端代码后自己打开页面、截图、读 console/network 验证效果。
2. **通用网页自动化** — 点击、填表、导航、抓取内容。
3. **操作用户已登录的浏览器** — 在用户真实 Chrome（带登录态）上执行操作。
4. **无头抓取/测试** — headless 模式跑页面测试、批量抓取。

## 非目标

- 不做站点级权限系统（v1 复用标准 `ApprovalGate` 决策通道
  Allow/Always/Deny，浏览器工具外面套自定义 gate 装饰器注入上下文，
  见"安全与审批模型"；站点级 allow 规则留待后续）。
- 不隐藏 `chrome.debugger` 的"正在被调试"横幅。
- 不引入 JS 构建工具链到 zode 仓库（扩展用 vanilla JS）。
- 不支持 Firefox/Safari（chromium 系可通过 `browser.executable` 覆盖）。

## 关键决策（已确认）

| 决策点 | 结论 | 理由 |
|---|---|---|
| 登录态方案 | 分期：M1 专用持久化 profile；M2 扩展桥接真实 Chrome | Chrome 136+ 禁止在默认 profile 上开 CDP 远程调试；`chrome.debugger` 扩展 API 是接管真实浏览器的官方许可通道 |
| CDP 库 | chromiumoxide 0.9.1（MIT OR Apache-2.0） | 原生 tokio async，贴合 `Tool::call` 异步模型；事件是 async stream；对比过 headless_chrome 1.0.22（阻塞 API，需 spawn_blocking，取消/超时别扭） |
| 工具粒度 | 粗粒度四工具 browser_read / browser_act / browser_eval / browser_tabs | 沿用 op_read/op_write 按安全类拆工具的先例，审批模型映射干净；evaluate 独立成工具以隔离 always-allow 范围（Codex 审查修订） |
| 桥接传输 | 本地 WebSocket + 配对码 | 与 openpencil 的 localhost 信任边界风格一致，零安装步骤；否掉 Native Messaging（要装 manifest、多一层 IPC） |

## 架构

新增 `zode-core/src/browser/` 模块（镜像 `openpencil/` 分层）+ 仓库顶层 `extensions/chrome/`：

```text
zode-core/src/browser/
├── mod.rs          公共类型、BrowserConfig、re-export
├── backend.rs      BrowserBackend trait（语义操作层）
├── managed.rs      ManagedBackend：chromiumoxide + 专用持久化 profile
├── bridge/
│   ├── mod.rs      配对状态、token 持久化（~/.zode/browser-bridge.json，0600）
│   ├── server.rs   BridgeServer：127.0.0.1 WS 监听 + 配对码校验
│   └── backend.rs  BridgeBackend：语义操作 → CDP JSON，经 WS 隧道给扩展
├── session.rs      BrowserSession：当前 target（managed|bridge）、惰性初始化、tab 状态
└── tools.rs        browser_read / browser_act / browser_eval / browser_tabs

zode-core/src/commands/browser.rs    /browser 斜杠命令解析器
zode-tui/src/ui/dialog/browser.rs    /browser 交互面板（无参数时打开）
extensions/chrome/                   MV3 扩展：manifest.json + background.js + popup
```

**统一层的抽象高度**：`BrowserBackend` trait 定义约 15 个**语义操作**
（navigate / screenshot / click / type_text / key / scroll / evaluate /
snapshot / console_logs / network_log / tab 管理），不是裸 CDP。

- ManagedBackend 用 chromiumoxide 类型化 API 实现。
- BridgeBackend 发 `{method, params}` JSON，扩展端 `chrome.debugger.sendCommand` 执行。
- 工具层只面对 trait；target 可运行时切换（`/browser target managed|bridge`，
  默认值 `browser.defaultTarget`）。

**`BrowserSession` 所有权与并发**（Codex 审查修订）：

- **进程级全局**：一个 zode 进程一个 `Arc<BrowserSession>`，作为 `EngineTemplate`
  的显式字段，组装每个 tab 引擎时 clone 进工具依赖。所有 zode tab 共享同一浏览器实例。
  与每 tab 的 `CarryState`（engine.rs:269）无关，tab 重组不影响浏览器状态。
  注意 `OpConnection` 是无状态 unit struct（每次调用 `ensure`），**不是**本设计的先例；
  BrowserSession 是长活状态，所有权必须如上显式定义。
- **操作串行化**：agent 的工具调用是并发派发的（默认最多 8 个并发），而
  "当前 tab / 当前 target" 是会话状态。`BrowserSession` 内部持一把 async Mutex
  将全部后端操作串行执行，杜绝 `select`/`click`/`screenshot` 之间的竞态。

## 上游 agent-rs 补口（截图回传前置条件）

QueryLoop 目前把一切工具输出 `to_string()` 成 `ToolResultContent::Text`
（`vendor/agent/crates/agent/src/query/loop_.rs:828`），图片无法回传模型；
FileRead 也不支持图片。补一个向后兼容的哨兵约定（按 Codex 审查收紧）：

- 保留键用抗碰撞形式 `"__agent_content_blocks__"`，要求**精确形状**：顶层对象只含
  该键 + 可选 `"text"` 键，值为 ContentBlock JSON 数组。
- **只允许 `Text` / `Image` 两种 block**（`ContentBlock` 还有
  ToolUse/ToolResult/Thinking 变体，Anthropic 的 tool_result 只干净接受
  text/image）；出现其他变体则整体回退 Text 路径。
- 只对 `ok: true` 的工具结果解析哨兵；失败结果一律走 Text。
- 沿用 `MAX_INLINE_IMAGE_BYTES`（5 MiB，attachments/mod.rs:100）做尺寸上限，
  超限回退为文本（附落盘路径）——哨兵不得绕过既有内联图片上限。
- 改动提交到 agent-rs 上游（vendor/agent 子模块），符合 "feed gaps back" 惯例，附单元测试。

**Provider 门控**（Codex 审查修订，二轮收紧谓词）：门控谓词是
**"provider 的 tool_result 渲染保留 image block"**，而非泛化的能力位 ——
openai_compat 对 tool_result Blocks 的拍平（`openai_compat.rs:459` →
`content_to_plain_text`）是无条件的，与其 `supports_images` 配置无关
（该位管用户消息图片）。当前满足谓词的只有 Anthropic 渲染路径。
实现上在 provider 能力元数据里加一个明确的
`tool_result_images`（或等价）标志，Anthropic 置 true；其余 provider
工具结果只带落盘路径文本，不做无效的图片内联。M1 验收判据按此表述。

截图实现：CDP `Page.captureScreenshot`（JPEG/WebP，质量 ~70），视口固定为配置值
（默认 1280×800），**不引入 image crate**。截图始终落盘会话目录，
工具结果文本部分附文件路径，方便用户自行打开。

## 工具面

| 工具 | actions | 安全类 | 审批 |
|---|---|---|---|
| `browser_read` | `screenshot` / `snapshot` / `console` / `network` / `tabs`（列表） | ReadOnly | 免审批 |
| `browser_act` | `navigate` / `click` / `type` / `key` / `scroll` | Mutating | ApprovalGate，可 always-allow |
| `browser_eval` | 执行任意 JS（`expression`） | Mutating | ApprovalGate，**独立** always-allow 范围 |
| `browser_tabs` | `new` / `close` / `select` | Mutating | ApprovalGate |

- **`evaluate` 独立成 `browser_eval`**（Codex 审查修订）：`PermissionGatedTool`
  的 always-allow 按工具包装器缓存（gated_tool.rs:19 的 `AtomicBool`），若与
  click/navigate 同锅，用户对一次无害点击选 "always" 就等于放行了后续任意 JS。
  拆开后各自独立授权。
- tab **列表**归入 `browser_read`（避免按输入分流审批）；`browser_tabs` 只留改状态动作。
- 点击定位三种方式：CSS selector、上次 `snapshot` 返回的元素 ref、x/y 坐标。
- `snapshot` 通过注入 JS 生成带 ref 的精简 DOM 大纲。
- 四工具注册进 `plugin.rs` 的 `browser` 工具组，可整组开关。

## 数据流

**Managed**：工具调用 → `session.ensure()` 惰性启动 Chrome（自动探测
Chrome/Chromium/Edge 可执行文件；`--user-data-dir=~/.zode/browser-profile`；
默认 headed，`browser.headless` 可改）→ 执行语义操作 → 结果回传。
用户在该 profile 登录过的站点 Cookie 永久保留（M1 的登录态方案）。

chromiumoxide 生命周期（Codex 审查修订）：`Browser::launch` 返回
`(Browser, Handler)` 二元组，`Handler` 是必须持续轮询的事件流 ——
`BrowserSession` 启动一个 **supervisor task** 专职驱动它；supervisor 退出
（浏览器崩溃/被用户手动关闭）时置 session 为 dead，下次工具调用触发一次自动重启。
关停：zode 退出时 managed 实例随之关闭（headed 模式下询问用户）。取消语义：
turn 被 Esc 取消只是 drop 等待结果的 future，已发出的 CDP 命令在浏览器侧
照常完成 —— spec 明确这是"放弃等待"而非"撤销操作"。

**Bridge**：`/browser pair` → BridgeServer 起监听 → TUI 展示 6 位配对码 →
用户点扩展图标输入 → WS 握手换发长期 token（`chrome.storage` 持久化，自动重连）→
工具调用经 WS RPC 隧道 → `chrome.debugger` 在真实 Chrome 上执行。

Bridge 运行时细则（Codex 审查修订）：

- **配对码**：TTL 2 分钟、单次有效、连续 5 次错误即作废并要求重新 `/browser pair`。
- **长期 token**：256-bit CSPRNG 生成，常量时间比较；Origin
  （`chrome-extension://<id>`）只作纵深防御，**不作为认证**（RFC 6455 下
  Origin 对非浏览器客户端不可信）。WS 只绑 127.0.0.1。
- **MV3 service worker 生命周期**：SW 30 秒可能被挂起；扩展侧 WS 需按 Chrome
  文档的 keepalive 约定发心跳（<30s 间隔）维持连接，断开自动重连再走 token 握手。
- **`chrome.debugger` 不是透明 CDP 隧道**：可用域受限、frame/worker/target 路由
  有差异。实施计划中必须给出语义操作 × bridge 的逐项映射矩阵；
  监听 `onDetach`（用户关 tab / 打开 DevTools 时触发），映射为明确的工具错误。

**`/browser` 斜杠命令**（用户需求变更：面板体验对齐 Claude Code 的 `/chrome`，命令名统一用 browser）：

- **无参数 `/browser`** → 打开交互面板（`ui/dialog/browser.rs`，沿用 TUI 现有
  dialog 基建），布局参照 Claude Code：

  ```text
  浏览器控制
  说明一段（导航/填表/截图/console/network 调试）

  Status:    Enabled | Disabled        ← browser 工具组启用状态
  Target:    managed | bridge          ← 当前后端
  Extension: Paired | Not paired       ← bridge 配对状态

  › Select target…                     ← 切换 managed / bridge
    Manage permissions                 ← 查看/重置浏览器工具 always-allow 状态
    Reconnect extension                ← 重新配对 / 重连 bridge
    Enabled by default: Yes            ← 切换工具组默认启停（写回 config）

  Usage: zode --browser or zode --no-browser
  ```

  Up/Down 选择、Enter 执行、Esc 关闭，与 settings/mcp 等既有 dialog 交互一致。
- **子命令保留**（脚本化/快速路径）：`status` / `launch` / `close` / `pair` /
  `target <managed|bridge>` / `screenshot [path]`。
- 解析器（`commands/browser.rs`）、TUI 处理、自动补全照抄 `/op` 三件套模式。
- **CLI 标志**：`zode --browser` / `zode --no-browser` 本次会话强制启/停
  browser 工具组（不写回配置）。
- 命名口径：命令、工具名、配置键统一为 `browser`（兼容 Chromium/Edge，
  语义准确）；仅扩展目录叫 `extensions/chrome/`（MV3 是 Chrome 平台产物）。

## 配置

`~/.zode/config.json`（camelCase，与 `openpencil` 键同级，全部可省略）：

```json
{
  "browser": {
    "executable": null,
    "headless": false,
    "profileDir": null,
    "defaultTarget": "managed",
    "viewport": { "width": 1280, "height": 800 }
  }
}
```

- `executable: null` → 按平台自动探测 Chrome/Chromium/Edge。
- `profileDir: null` → 默认 `~/.zode/browser-profile`。
- 接线遵循现有约定（Codex 审查修订）：`BrowserConfig` 定义在 `browser/mod.rs`，
  但必须作为字段挂进 `ZodeConfig` 并接入 `merge_from`（config.rs:932）的
  逐键合并，与 `openpencil` 键同构。

## 安全与审批模型

- **Profile 隔离**：managed 永不触碰用户日常 profile（Chrome 136+ 下 CDP
  在默认 profile 也已被禁，是设计选择也是硬约束）。profile 目录 0700。
- **Bridge 信任链**：配对码只在 TUI 展示、一次有效；token 存
  `~/.zode/browser-bridge.json`（0600）；每次连接校验 token +
  `chrome-extension://<id>` Origin；只绑 127.0.0.1。
- **审批**：`browser_act` / `browser_eval` / `browser_tabs` 过 ApprovalGate。
  `ApprovalGate::approve` 只接收 `(tool, input)`（approval.rs:20），而
  target/当前页 URL 是会话状态、不在模型输入里 —— 因此浏览器工具用一个
  **自定义 gate 装饰器**（替代通用 `PermissionGatedTool`）：送审前把只读上下文
  （target、当前 URL）注入 gate 可见的 input 副本（不改传给内层工具的 input），
  并为 browser 工具补 `summarize_input` 专属 match 臂 + TUI 审批对话框的
  动作详情渲染。`browser_eval` 的 JS 表达式在弹窗中完整展示；
  bridge 模式额外标注"动作发生在用户真实浏览器"。
- **扩展权限最小化**：manifest 只声明 `debugger` / `tabs` / `storage`，
  无 host_permissions。调试横幅保留，作为用户可见信号。
- **明示边界**：免审批的 `browser_read` 在 bridge 模式能读到真实页面内容
  （含截图）——配对即授权读取。

## 错误处理

- **找不到 Chrome** → 工具报错，列出探测过的路径 + `browser.executable` 提示；
  `/browser status` 可自查。
- **启动失败/崩溃** → 每次工具调用最多自动重启一次；仍失败以 `ok:false`
  工具错误浮出，不 panic、不阻塞 turn。
- **Bridge 未连接** → 指引 "run `/browser pair` 并点击扩展图标"。
- **Bridge 中途脱落**（`chrome.debugger.onDetach`：用户关 tab、打开 DevTools）
  → 当次调用以明确的工具错误返回，附原因与恢复指引。
- **超时**：单操作 30s 上限；navigate 等 load 事件、10s 封顶后带已有状态返回
  （不算失败）。async future 随 turn 取消（Esc）直接 drop；已发出的 CDP
  命令不撤销（见 Managed 生命周期）。

## 测试

- **单元（CI 全跑）**：`/browser` 解析器（含面板动作分发）；config 解析与默认值；`MockBackend`
  驱动四工具（action 分发 / schema 校验 / 安全类断言 / always-allow 范围隔离）；
  哨兵形状校验（拒非法变体、超限回退）；bridge 协议帧
  （配对码、token 校验、Origin 拒绝）用进程内 WS 客户端模拟扩展。
- **集成（`#[ignore]`，`ZODE_BROWSER_IT=1` 启用）**：真实 headless Chrome →
  navigate `data:` URL → screenshot 非空 → evaluate `1+1`。CI 不依赖 Chrome。
- **上游**：`__agent_content_blocks__` 哨兵转换的单测随 agent-rs 提交。
- **扩展**：逻辑压到最薄（收 RPC → debugger 转发），v1 手工测试清单，
  不引入 JS 测试工具链。

## 里程碑

| | 内容 | 交付判据 |
|---|---|---|
| **M1** | agent-rs 哨兵补口 + `browser/` 模块（managed 后端 + supervisor）+ 四工具 + 自定义 gate + `/browser`（交互面板 + 子命令三件套 + `--browser`/`--no-browser` 标志）+ 配置接线 + 测试 | agent 能自主打开页面、读 console、点击输入验证前端改动；`tool_result_images`（tool_result 渲染保留 image block）的 provider（当前即 Anthropic）额外能把截图给模型看，其余 provider 得到落盘路径 |
| ✅ **M2** | BridgeServer（含配对硬化）+ `extensions/chrome/` 扩展 + 语义操作×bridge 映射矩阵 + onDetach 处理 + target 切换 + 扩展安装/更新说明与固定扩展 id（Origin/token 锚定） | 配对后 agent 能在用户真实 Chrome（带登录态）上执行同一套工具 |

## 新增依赖

- `chromiumoxide = "0.9"` — zode-core。默认特性即可（0.9.1 无
  `tokio-runtime`/`async-std-runtime` 特性开关，Tokio 原生支持；默认特性仅
  `bytes`）。其传递引入的 WS/base64 依赖**不得**当作直接依赖使用。
- WS server（bridge 用）：显式添加 `tokio-tungstenite`（不依赖传递依赖）。
