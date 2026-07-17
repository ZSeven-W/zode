use std::{path::PathBuf, process::Command};

use zode_app_model::{
    reduce_settings_command, AppCommand, LoadState, LocalSettingFact, SettingsCategory,
    SettingsCommandOutcome, ShellRoute, ZodeAppState,
};
use zode_app_runtime::workspace_uri_to_path;
use zode_app_ui::{Key, KeyEvent, Modifiers, SettingsPanel, SETTINGS_SEARCH_ID};
use zode_core::config::{ConfigManager, ZodeConfig};

use super::DesktopApp;

pub(super) fn reduce_local_settings_command(
    state: &mut ZodeAppState,
    command: AppCommand,
) -> SettingsCommandOutcome {
    if !matches!(
        command,
        AppCommand::SetProjectPermissions { .. }
            | AppCommand::SetThemePreference(_)
            | AppCommand::SetReducedMotion(_)
            | AppCommand::SetHighContrast(_)
            | AppCommand::SetSettingsSearch(_)
            | AppCommand::SetSettingsScroll { .. }
    ) {
        return SettingsCommandOutcome::Ignored;
    }
    reduce_settings_command(state, command)
}

pub(super) fn settings_interaction_viewport(
    snapshot: &zode_app_ui::WorkspaceSnapshot,
) -> jian_widgets::Rect {
    SettingsPanel::page_layout(snapshot.layout.primary_surface).0
}

impl DesktopApp {
    pub(super) fn refresh_local_settings_for_category(&mut self, category: SettingsCategory) {
        let cwd = settings_workspace_path(&self.app_state);
        match category {
            SettingsCategory::Configuration => {
                self.app_state.local_settings.configuration = load_config_facts(cwd.as_deref());
            }
            SettingsCategory::Browser => {
                self.app_state.local_settings.browser = load_browser_facts(cwd.as_deref());
            }
            SettingsCategory::Hooks => {
                self.app_state.local_settings.hooks = load_hook_facts(cwd.as_deref());
            }
            SettingsCategory::Git => {
                self.app_state.local_settings.git = load_git_facts(cwd.as_deref());
            }
            SettingsCategory::Worktree => {
                self.app_state.local_settings.worktrees = load_worktree_facts(cwd.as_deref());
            }
            _ => {}
        }
    }

    pub(super) fn handle_settings_search_key(&mut self, event: &KeyEvent) -> bool {
        if !event.pressed
            || !matches!(self.app_state.presentation.route, ShellRoute::Settings(_))
            || self.focused_widget != Some(SETTINGS_SEARCH_ID)
        {
            return false;
        }
        let mut query = self.app_state.settings_search.clone();
        match &event.key {
            Key::Escape if !query.is_empty() => query.clear(),
            Key::Escape => return false,
            Key::Backspace => {
                query.pop();
            }
            Key::Delete => query.clear(),
            Key::Character(value)
                if event.modifiers.primary() && value.eq_ignore_ascii_case("a") =>
            {
                query.clear();
            }
            Key::Character(value)
                if !event.modifiers.primary() && !event.modifiers.contains(Modifiers::ALT) =>
            {
                query.push_str(value);
            }
            Key::Enter
            | Key::ArrowLeft
            | Key::ArrowRight
            | Key::ArrowUp
            | Key::ArrowDown
            | Key::Home
            | Key::End
            | Key::PageUp
            | Key::PageDown => return true,
            Key::Tab | Key::Character(_) => return false,
        }
        self.set_settings_search_value(query);
        true
    }

    pub(super) fn handle_settings_search_ime(&mut self, event: &zode_app_ui::ImeEvent) -> bool {
        if !matches!(self.app_state.presentation.route, ShellRoute::Settings(_))
            || self.focused_widget != Some(SETTINGS_SEARCH_ID)
        {
            return false;
        }
        if let zode_app_ui::ImeEvent::Commit(text) = event {
            let mut query = self.app_state.settings_search.clone();
            query.push_str(text);
            self.set_settings_search_value(query);
        }
        true
    }

    pub(super) fn paste_settings_search_text(&mut self, text: &str) -> bool {
        if !matches!(self.app_state.presentation.route, ShellRoute::Settings(_))
            || self.focused_widget != Some(SETTINGS_SEARCH_ID)
        {
            return false;
        }
        let mut query = self.app_state.settings_search.clone();
        query.push_str(text);
        self.set_settings_search_value(query);
        true
    }

    pub(super) fn set_settings_search_value(&mut self, value: String) {
        if reduce_settings_command(&mut self.app_state, AppCommand::SetSettingsSearch(value))
            == SettingsCommandOutcome::Applied
        {
            self.rebuild_frame_snapshot();
            self.request_redraw();
        }
    }

    pub(super) fn apply_settings_scroll_delta(&mut self, delta: f32) {
        let command = SettingsPanel::scroll_command(
            settings_interaction_viewport(&self.frame_snapshot),
            &self.app_state,
            delta,
        );
        if reduce_settings_command(&mut self.app_state, command) == SettingsCommandOutcome::Applied
        {
            self.rebuild_frame_snapshot();
            self.request_redraw();
        }
    }
}

fn settings_workspace_path(state: &ZodeAppState) -> Option<PathBuf> {
    let workspace = state
        .active_available_workspace()
        .or_else(|| {
            state
                .current_session
                .as_ref()
                .and_then(|session| state.available_workspace_for_session(session))
        })
        .filter(|workspace| !state.is_projectless_workspace(workspace));
    workspace
        .and_then(|workspace| workspace_uri_to_path(workspace).ok())
        .or_else(|| ConfigManager::config_dir().ok())
}

fn load_config(cwd: Option<&std::path::Path>) -> Result<(PathBuf, ZodeConfig), String> {
    let config_dir = ConfigManager::config_dir().map_err(|error| error.to_string())?;
    let config_path = ConfigManager::global_path_in(&config_dir);
    let config = match cwd {
        Some(cwd) => ConfigManager::load(cwd),
        None => ConfigManager::load_global(),
    }
    .map_err(|error| error.to_string())?;
    Ok((config_path, config))
}

fn load_config_facts(cwd: Option<&std::path::Path>) -> LoadState<Vec<LocalSettingFact>> {
    match load_config(cwd) {
        Ok((path, config)) => LoadState::Ready(vec![
            fact("配置文件", path.display()),
            fact("Provider", format!("{:?}", config.provider.kind())),
            fact("模型", config.provider.model.as_deref().unwrap_or("未配置")),
            fact("语言", config.language.as_deref().unwrap_or("未配置")),
            fact("推理模式", config.effort.as_deref().unwrap_or("未配置")),
            fact(
                "批准设置",
                format!(
                    "允许 {} · 询问 {} · 拒绝 {}",
                    config.permissions.allow.len(),
                    config.permissions.ask.len(),
                    config.permissions.deny.len()
                ),
            ),
            fact(
                "沙箱",
                if config.sandbox.enabled == Some(false) {
                    "已关闭"
                } else {
                    config.sandbox.mode.as_deref().unwrap_or("workspace-write")
                },
            ),
            fact(
                "沙箱连接",
                if config.sandbox.network.unwrap_or(false) {
                    "允许"
                } else {
                    "关闭"
                },
            ),
            fact(
                "自动编排",
                if config.autonomous_orchestration.unwrap_or(true) {
                    "打开"
                } else {
                    "关闭"
                },
            ),
        ]),
        Err(error) => LoadState::Failed(error),
    }
}

fn load_browser_facts(cwd: Option<&std::path::Path>) -> LoadState<Vec<LocalSettingFact>> {
    match load_config(cwd) {
        Ok((_, config)) => {
            let profile = config.browser.profile_dir.clone().or_else(|| {
                ConfigManager::config_dir()
                    .ok()
                    .map(|dir| dir.join("browser-profile").display().to_string())
            });
            let (width, height) = config.browser.viewport();
            LoadState::Ready(vec![
                fact(
                    "内置浏览器",
                    if config.browser.enabled() {
                        "打开"
                    } else {
                        "禁用"
                    },
                ),
                fact("默认目标", config.browser.default_target()),
                fact(
                    "运行模式",
                    if config.browser.headless() {
                        "无头"
                    } else {
                        "有界面"
                    },
                ),
                fact(
                    "浏览器程序",
                    config.browser.executable.as_deref().unwrap_or("自动发现"),
                ),
                fact("独立资料目录", profile.as_deref().unwrap_or("不可用")),
                fact("视图", format!("{width} × {height}")),
            ])
        }
        Err(error) => LoadState::Failed(error),
    }
}

fn load_hook_facts(cwd: Option<&std::path::Path>) -> LoadState<Vec<LocalSettingFact>> {
    let Some(cwd) = cwd else {
        return LoadState::Ready(Vec::new());
    };
    LoadState::Ready(
        zode_core::hooks_config::load_hook_entries(cwd)
            .into_iter()
            .map(|hook| {
                fact(
                    match hook.tool {
                        Some(tool) => format!("{} · {tool}", hook.event),
                        None => hook.event,
                    },
                    hook.script,
                )
            })
            .collect(),
    )
}

fn load_git_facts(cwd: Option<&std::path::Path>) -> LoadState<Vec<LocalSettingFact>> {
    let Some(cwd) = cwd.filter(|cwd| cwd.is_dir()) else {
        return LoadState::Ready(Vec::new());
    };
    let root = match git_output(cwd, &["rev-parse", "--show-toplevel"]) {
        Ok(root) => root,
        Err(error) => return LoadState::Failed(error),
    };
    let branch = zode_core::instructions::detect_git_branch(cwd)
        .filter(|branch| !branch.trim().is_empty())
        .unwrap_or_else(|| "detached HEAD".into());
    LoadState::Ready(vec![fact("仓库根目录", root), fact("当前分支", branch)])
}

fn load_worktree_facts(cwd: Option<&std::path::Path>) -> LoadState<Vec<LocalSettingFact>> {
    let Some(cwd) = cwd.filter(|cwd| cwd.is_dir()) else {
        return LoadState::Ready(Vec::new());
    };
    match git_output(cwd, &["worktree", "list", "--porcelain"]) {
        Ok(output) => LoadState::Ready(parse_worktrees(&output)),
        Err(error) => LoadState::Failed(error),
    }
}

fn git_output(cwd: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if message.is_empty() {
            format!("git exited with {}", output.status)
        } else {
            message
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn parse_worktrees(output: &str) -> Vec<LocalSettingFact> {
    let mut result = Vec::new();
    let mut path = None::<String>;
    let mut branch = None::<String>;
    let push = |result: &mut Vec<LocalSettingFact>,
                path: &mut Option<String>,
                branch: &mut Option<String>| {
        let Some(current_path) = path.take() else {
            branch.take();
            return;
        };
        let label = std::path::Path::new(&current_path)
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("工作树")
            .to_owned();
        let branch = branch
            .take()
            .map(|branch| branch.trim_start_matches("refs/heads/").to_owned())
            .unwrap_or_else(|| "detached HEAD".into());
        result.push(fact(label, format!("{branch} · {current_path}")));
    };
    for line in output.lines().chain(std::iter::once("")) {
        if line.is_empty() {
            push(&mut result, &mut path, &mut branch);
        } else if let Some(value) = line.strip_prefix("worktree ") {
            path = Some(value.to_owned());
        } else if let Some(value) = line.strip_prefix("branch ") {
            branch = Some(value.to_owned());
        }
    }
    result
}

fn fact(label: impl Into<String>, value: impl ToString) -> LocalSettingFact {
    LocalSettingFact {
        label: label.into(),
        value: value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_worktrees;

    #[test]
    fn porcelain_worktrees_are_projected_without_inventing_branches() {
        let facts = parse_worktrees(
            "worktree /repo/zode\nHEAD abc\nbranch refs/heads/main\n\nworktree /tmp/detached\nHEAD def\ndetached\n",
        );
        assert_eq!(facts.len(), 2);
        assert_eq!(facts[0].label, "zode");
        assert_eq!(facts[0].value, "main · /repo/zode");
        assert_eq!(facts[1].value, "detached HEAD · /tmp/detached");
    }
}
