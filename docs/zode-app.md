# Zode Desktop（首版预览）

Zode Desktop 是与 `zode` CLI/TUI 共用 Rust agent runtime 的本机桌面入口。首版以本机工作区、同进程 `LocalAgentEndpoint` 和可恢复 session 为边界；它不是远程控制客户端，也尚未提供移动端应用。

## 安装与启动

从源码启动需要 Rust 1.94，并且仓库必须包含子模块：

```bash
git clone --recurse-submodules https://github.com/ZSeven-W/zode.git
cd zode
cargo +1.94 run -p zode-app
```

无窗口检查可渲染固定尺寸 PNG：

```bash
cargo +1.94 run -p zode-app -- --render-snapshot /tmp/zode-app.png
```

发布构建会生成以下原生制品：

- macOS：`Zode.app` 与 tar 包；当前仅 ad-hoc 签名，未 notarize。
- Windows：便携 zip 与 WiX MSI；当前未做 Authenticode 签名。
- Linux：AppImage 与便携 tar 包；当前未做发行方签名。

这些制品由 `scripts/build-app-release.sh` 生成；源码启动仍是开发阶段最直接的查看方式。

## 与 CLI 共享的状态

桌面端和 CLI 默认共用 `~/.zode`。设置 `ZODE_CONFIG_DIR` 可以把整套全局状态指向另一个目录，适合测试或隔离配置。

| 路径 | 作用 |
| --- | --- |
| `~/.zode/config.json` | provider、model、sandbox、browser 等全局配置 |
| `~/.zode/node.json` | 本机 node 的稳定身份；重启后保持不变 |
| `~/.zode/sessions/index.json` | session 索引 |
| `~/.zode/sessions/<id>.jsonl` | 可恢复的对话与运行时消息 |
| `~/.zode/app-state.json` | 窗口、主题、侧栏和 session UI 状态 |
| `<project>/.zode/state.json` | 项目级权限与 sandbox 等机器维护状态 |

不要手工改写运行中的 session JSONL 或 `node.json`。项目级 `state.json` 由 Zode 管理；需要撤销永久权限时，优先使用桌面设置中的权限页。

## 权限语义

有副作用的工具调用仍经过与 CLI 相同的审批和 sandbox 边界：

- `Allow once`：只放行当前请求，不落盘。
- `Allow always`：按工具名写入当前项目的 `.zode/state.json`，只对该项目生效。
- `Deny`：拒绝当前请求。
- 如果项目权限无法写入，`Allow always` 会显式降级为本次放行，不会伪装成已永久保存。

审批不是 sandbox 的替代品。即使工具已被项目允许，shell 仍受当前 read-only/workspace-write、可写目录和网络策略约束；网络默认不因永久审批而自动开放。

## 键盘操作

`Primary` 在 macOS 上是 `Cmd`，在 Windows/Linux 上是 `Ctrl`。

| 按键 | 行为 |
| --- | --- |
| `Tab` / `Shift+Tab` | 在可聚焦控件之间前进/后退 |
| `Enter` / `Space` | 激活当前非输入控件 |
| 编辑器内 `Enter` | 发送；turn 运行中则作为 steer 输入 |
| 编辑器内 `Shift+Enter` | 插入换行 |
| `Primary+A` | 全选编辑器文本 |
| `Primary+V` | 粘贴文本或受支持的图片 |
| `Primary+\`` | 打开或切换本机终端页 |
| 设置页 `Esc` | 返回对话页 |
| 设置页 `PageUp/PageDown/Home/End` | 翻页或跳到开头/末尾 |

终端获得焦点后，macOS 使用 `Cmd+C` / `Cmd+V` 复制粘贴；Windows/Linux 使用 `Ctrl+Shift+C` / `Ctrl+Shift+V`。普通 `Ctrl+C` 会发送给 PTY 内的前台进程。

## 内置终端的 VT 边界

首版终端基于本机 PTY 和 `vte` 解析器，支持常用文本输入、光标移动、清屏、滚动区域、粗体、基础色、256 色和 RGB 色，以及最多 10,000 行 scrollback。它不是完整的 xterm 仿真：

- 调整窗口宽度时只截断或补齐现有行，不做 xterm 风格的内容重排。
- 未实现的 CSI、ESC、OSC 和私有模式会被忽略。
- 依赖 alternate screen、鼠标上报或复杂终端能力的全屏程序可能显示不完整。

遇到兼容性问题时，可继续使用系统终端运行 `zode` TUI。

## 当前未实现

- 远程 `PeerAgentEndpoint`、跨机器 session 和远程审批。
- iOS/Android 原生应用。CI 的移动 target 只验证可移植协议、model 和 UI core 能编译，不代表移动产品已交付。
- 对移动端相机、系统分享、后台运行和触屏专用导航的产品集成。

## 本地验收

以下脚本只在最底层 session-engine seam 注入确定性 fake provider，其余仍经过生产 `AppBootstrap`、`LocalAppRuntime`、`EngineBackend`、`ZodeEngineDriver` 和 `LocalSessionRepository`；它不访问 provider 网络，也不会打开交互窗口。最后会生成 `${TMPDIR:-/tmp}/zode-app-smoke.png`：

```bash
scripts/test-app-smoke.sh
```

端到端测试覆盖创建 session、文本/工具/usage 事件、永久审批、真实 git diff、落盘恢复，以及 interrupt 在 endpoint/node 重建后不重放。
