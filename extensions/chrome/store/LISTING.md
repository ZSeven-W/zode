# Chrome Web Store listing — zode bridge

Copy the fields below into the Web Store developer dashboard. The permission
justifications are required for review (this extension uses sensitive
`debugger` and `nativeMessaging` permissions).

---

## Name

zode bridge

## Short description (English, ≤132 chars)

Let the zode coding agent see and drive the page you're on — ask about it in a
side panel, or hand it a task to click, type, and read.

## Short description (简体中文, ≤132 chars)

让本地 zode 编码助手看到并操作你正在浏览的页面——在侧边栏就当前页提问,或交给它点击、输入、读取页面的任务。

---

## Detailed description (English)

zode bridge connects your Chrome to the **zode** CLI running on your own
machine, so its AI coding agent can work with the page you're actually looking
at — not a fresh headless tab.

**What you can do**

- **Ask about this page.** Open the side panel (Alt+Z) and ask a question about
  the current tab. The agent reads the page and answers — no copy-pasting the
  URL or the text.
- **Point at an element.** Use the element picker to attach the exact button,
  field, or block you mean, and the agent acts on precisely that.
- **Hand it a task.** "Fill this form", "click through to the next page and
  summarize", "grab the console errors" — the agent navigates, clicks, types,
  scrolls, screenshots, and reads console/network output for you.
- **Keep your session.** It drives your real, logged-in Chrome tab, so pages
  behind a login just work — no separate browser, no re-authenticating.

**Requires the zode CLI.** This extension is a bridge, not a standalone app.
Install zode from https://github.com/ZSeven-W/zode first, then run
`/browser pair` in zode and enter the pairing code shown in the extension.

**Privacy & security**

- The extension talks ONLY to zode on `127.0.0.1` (localhost) over a WebSocket
  secured by a 6-digit pairing code and a long-term token stored on your
  machine. Nothing is sent to any third-party server by the extension itself.
- The agent runs locally in your zode CLI; the only data that leaves your
  machine is whatever you send to the AI model you configured in zode.
- Page control only happens on the tab you pair, on your explicit action. The
  extension never captures a password field's value.

Open source — the full extension and CLI source, including exactly what each
permission is used for, is at https://github.com/ZSeven-W/zode.

## Detailed description (简体中文)

zode bridge 把你的 Chrome 与运行在**本机**的 **zode** 命令行工具连接起来,让它的
AI 编码助手能直接处理你正在看的页面,而不是另开一个无头标签页。

**能做什么**

- **就当前页提问。** 打开侧边栏(Alt+Z)对当前标签页提问,助手会读取页面并回答——
  不用复制 URL 或正文。
- **指定页面元素。** 用元素选择器把你指的那个按钮、输入框或区块精确附上,助手就针对它操作。
- **交给它一个任务。** "填这个表单""翻到下一页并总结""抓一下控制台报错"——助手会为你
  导航、点击、输入、滚动、截图,并读取控制台/网络输出。
- **保留登录态。** 它驱动的是你真实的、已登录的 Chrome 标签页,登录后的页面直接可用——
  不用另开浏览器,也不用重新登录。

**需要先安装 zode CLI。** 本扩展是桥接,不是独立应用。请先从
https://github.com/ZSeven-W/zode 安装 zode,在 zode 中运行 `/browser pair`,再在扩展里
输入显示的配对码。

**隐私与安全**

- 扩展只与本机 `127.0.0.1`(localhost)上的 zode 通过 WebSocket 通信,由 6 位配对码和存在
  本机的长期令牌保护。扩展本身不向任何第三方服务器发送数据。
- 助手在你本地的 zode CLI 中运行;离开本机的数据只有你发给自己在 zode 中配置的 AI 模型的内容。
- 页面操作只发生在你配对的那个标签页、由你主动发起。扩展从不读取密码输入框的值。

开源——完整的扩展与 CLI 源码(包括每项权限的具体用途)见
https://github.com/ZSeven-W/zode。

---

## Permission justifications (for review — English)

- **debugger** — The agent drives the page (navigate, click, type, scroll,
  screenshot, read console/network) through the Chrome DevTools Protocol on the
  single tab the user paired. This is the core function.
- **nativeMessaging** — Lets the side panel start the local zode process on
  demand and re-establish the localhost bridge when it isn't already running.
- **tabs / tabGroups / webNavigation** — Target the paired tab, keep zode's own
  background tab in its own group, and hand control back to the user when they
  navigate the tab manually.
- **storage** — Persist the pairing token so the extension reconnects to the
  local zode without re-pairing every time.
- **alarms** — Periodic local reconnect check while disconnected, and
  self-opening the pairing page (Chrome blocks externally launched
  chrome-extension:// URLs). Local connections to 127.0.0.1 only.
- **downloads** — Report files the agent's actions caused the page to download,
  so the CLI can find them.
- **offscreen** — Detect the browser's light/dark theme to swap the toolbar
  icon; no page content is read.
- **sidePanel** — Hosts the "ask about this page" UI.
- **Host access** — Only `ws://127.0.0.1` (localhost) to reach the local zode
  CLI. No remote hosts.

## 权限用途说明(审核用 — 简体中文)

- **debugger** —— 助手通过 Chrome DevTools Protocol 在用户配对的那一个标签页上驱动页面
  (导航、点击、输入、滚动、截图、读取控制台/网络)。这是核心功能。
- **nativeMessaging** —— 让侧边栏在需要时启动本机 zode 进程,并在其未运行时重新建立
  localhost 桥接。
- **tabs / tabGroups / webNavigation** —— 定位配对标签页,把 zode 自己的后台标签页归入独立
  分组,并在用户手动导航该标签页时把控制权交还给用户。
- **storage** —— 保存配对令牌,以便扩展重连本机 zode 时无需每次重新配对。
- **alarms** —— 断线时约每 30 秒做一次本地重连检查,并在用户从 CLI 发起配对时由
  扩展自行打开配对页(Chrome 会拦截外部程序打开的 chrome-extension:// 地址)。
  仅连接 127.0.0.1,不访问网页、不传输数据。
- **downloads** —— 报告助手操作导致页面下载的文件,便于 CLI 找到它们。
- **offscreen** —— 检测浏览器明暗主题以切换工具栏图标;不读取任何页面内容。
- **sidePanel** —— 承载"就本页提问"的界面。
- **主机访问** —— 仅 `ws://127.0.0.1`(localhost)以连接本机 zode CLI,不访问任何远程主机。

---

## Per-field justifications — paste ONE per field on the Privacy practices tab

**Single purpose**

zode bridge has a single purpose: to bridge the user's Chrome browser to their
locally running zode command-line tool so its coding agent can read and control
the browser tab the user has explicitly paired.

**debugger**

The core function is letting the user's local zode coding agent drive the web
page they paired — navigate, click, type, scroll, capture screenshots, and read
console and network output — implemented through the Chrome DevTools Protocol,
which requires this permission. Control is limited to the single tab the user
explicitly pairs and happens only in response to the user's own instructions in
the zode CLI.

**nativeMessaging**

Used to start the user's locally installed zode process on demand and to
re-establish the localhost bridge to it when it is not already running. It
communicates only with the user's own zode CLI on their machine.

**tabs**

Needed to identify and target the specific tab the user paired for control, and
to activate the correct tab briefly when taking a screenshot before restoring
the previously active tab.

**tabGroups**

Keeps zode's own background helper tab in a dedicated tab group so it is
visually separated from the user's tabs and never mixed into their groups.

**webNavigation**

Lets the extension detect when the user manually navigates the controlled tab
(address bar, bookmark, link) so it hands control back to the user instead of
continuing to drive a page the user has moved away from.

**storage**

Persists the pairing token and minimal connection state locally so the
extension reconnects to the user's local zode CLI without repeating the pairing
step every session. No browsing data is stored.

**alarms**

Schedules a lightweight periodic check (about every 30 seconds, only while
disconnected) so the extension can silently re-establish the bridge to the
user's local zode CLI after it restarts, and can open the pairing page when
the user starts pairing from the CLI — Chrome blocks locally launched
chrome-extension:// URLs, so the extension must open that page itself. The
alarm triggers only local connection attempts to 127.0.0.1; it never accesses
web pages or transmits any data.

**downloads**

When an action the agent performs causes the page to download a file, the
extension reports that download event to the local zode CLI so the file can be
located. It does not start downloads on its own and does not read prior
download history.

**offscreen**

An offscreen document is used solely to detect the browser's light/dark color
scheme so the extension can swap its toolbar icon to match. No web page content
is accessed through it.

**sidePanel**

Hosts the extension's primary user interface: asking questions about the
current page and showing the agent's status and activity.

**Remote code use**

This extension does NOT use remotely hosted code. All HTML, CSS, and JavaScript
(including the bundled React side panel) are packaged inside the extension. It
loads no external scripts and evaluates no remote code; it only opens a
WebSocket to the user's local zode CLI on 127.0.0.1 to exchange JSON messages
(data, not code).
