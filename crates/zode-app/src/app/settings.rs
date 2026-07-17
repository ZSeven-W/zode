use std::{path::PathBuf, process::Command};

use zode_app_model::{
    reduce_settings_command, AppCommand, LoadState, LocalSettingFact, SettingsCategory,
    SettingsCommandOutcome, ShellRoute, ZodeAppState,
};
use zode_app_runtime::workspace_uri_to_path;
use zode_app_ui::{
    Key, KeyEvent, Modifiers, SettingsPanel, WidgetId, ARCHIVED_TASK_SEARCH_ID,
    COMPUTER_ALLOWED_APP_INPUT_ID, PROVIDER_API_KEY_INPUT_ID, PROVIDER_BASE_URL_INPUT_ID,
    PROVIDER_ID_INPUT_ID, PROVIDER_MODEL_IDS_INPUT_ID, SETTINGS_SEARCH_ID,
};
use zode_core::config::{ConfigManager, ZodeConfig};

use super::{provider_input::ProviderInputOutcome, DesktopApp};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchKeyOutcome {
    Changed,
    Consumed,
    Ignored,
}

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
            | AppCommand::SetTaskSuggestions(_)
            | AppCommand::SetSidebarTasksExpanded(_)
            | AppCommand::SetSettingsSearch(_)
            | AppCommand::SetArchivedTaskSearch(_)
            | AppCommand::SetArchivedTaskWorkspaceFilter(_)
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
            SettingsCategory::ComputerUse => {
                self.app_state.local_settings.computer =
                    super::computer_use::load_computer_use_snapshot();
            }
            SettingsCategory::ProviderModels => {
                self.provider_models_controller.refresh(&mut self.app_state);
            }
            _ => {}
        }
    }

    pub(super) fn handle_settings_search_key(&mut self, event: &KeyEvent) -> bool {
        if !event.pressed {
            return false;
        }
        if let Some(id) = self.focused_provider_input_id() {
            let outcome = self.provider_models_controller.edit_input_key(id, event);
            return self.apply_provider_input_outcome(id, outcome);
        }
        let Some((id, mut query)) = self.focused_settings_input() else {
            return false;
        };
        match edit_search_query(&mut query, event) {
            SearchKeyOutcome::Changed => self.set_settings_input_value(id, query),
            SearchKeyOutcome::Consumed => return true,
            SearchKeyOutcome::Ignored => return false,
        }
        true
    }

    pub(super) fn handle_settings_search_ime(&mut self, event: &zode_app_ui::ImeEvent) -> bool {
        if let Some(id) = self.focused_provider_input_id() {
            let outcome = self.provider_models_controller.edit_input_ime(id, event);
            return self.apply_provider_input_outcome(id, outcome);
        }
        let Some((id, mut query)) = self.focused_settings_input() else {
            return false;
        };
        if append_ime_commit(&mut query, event) {
            self.set_settings_input_value(id, query);
        }
        true
    }

    pub(super) fn paste_settings_search_text(&mut self, text: &str) -> bool {
        if let Some(id) = self.focused_provider_input_id() {
            let outcome = self.provider_models_controller.paste_input(id, text);
            return self.apply_provider_input_outcome(id, outcome);
        }
        let Some((id, mut query)) = self.focused_settings_input() else {
            return false;
        };
        query.push_str(text);
        self.set_settings_input_value(id, query);
        true
    }

    fn focused_settings_input(&self) -> Option<(WidgetId, String)> {
        match (self.app_state.presentation.route, self.focused_widget) {
            (ShellRoute::Settings(_), Some(SETTINGS_SEARCH_ID)) => {
                Some((SETTINGS_SEARCH_ID, self.app_state.settings_search.clone()))
            }
            (
                ShellRoute::Settings(SettingsCategory::ArchivedTasks),
                Some(ARCHIVED_TASK_SEARCH_ID),
            ) => Some((
                ARCHIVED_TASK_SEARCH_ID,
                self.app_state.archived_tasks.search.clone(),
            )),
            (
                ShellRoute::Settings(SettingsCategory::ComputerUse),
                Some(COMPUTER_ALLOWED_APP_INPUT_ID),
            ) => Some((
                COMPUTER_ALLOWED_APP_INPUT_ID,
                self.app_state.computer_use.allowed_app_input.clone(),
            )),
            _ => None,
        }
    }

    fn focused_provider_input_id(&self) -> Option<WidgetId> {
        match (self.app_state.presentation.route, self.focused_widget) {
            (ShellRoute::Settings(SettingsCategory::ProviderModels), Some(id))
                if is_provider_models_input(id) =>
            {
                Some(id)
            }
            _ => None,
        }
    }

    fn apply_provider_input_outcome(
        &mut self,
        id: WidgetId,
        outcome: ProviderInputOutcome,
    ) -> bool {
        match outcome {
            ProviderInputOutcome::Ignored => false,
            ProviderInputOutcome::Consumed => {
                self.rebuild_frame_snapshot();
                self.request_redraw();
                true
            }
            ProviderInputOutcome::TextChanged => {
                let Some(value) = self
                    .provider_models_controller
                    .input_value(id)
                    .map(ToOwned::to_owned)
                else {
                    return false;
                };
                self.project_provider_input_value(id, value);
                true
            }
        }
    }

    pub(super) fn set_settings_input_value(&mut self, id: WidgetId, value: String) {
        if matches!(
            self.app_state.presentation.route,
            ShellRoute::Settings(SettingsCategory::ProviderModels)
        ) && is_provider_models_input(id)
        {
            if self
                .provider_models_controller
                .set_input_value(id, value.clone())
            {
                self.project_provider_input_value(id, value);
            }
            return;
        }
        let command = match id {
            SETTINGS_SEARCH_ID => AppCommand::SetSettingsSearch(value),
            ARCHIVED_TASK_SEARCH_ID
                if matches!(
                    self.app_state.presentation.route,
                    ShellRoute::Settings(SettingsCategory::ArchivedTasks)
                ) =>
            {
                AppCommand::SetArchivedTaskSearch(value)
            }
            COMPUTER_ALLOWED_APP_INPUT_ID
                if matches!(
                    self.app_state.presentation.route,
                    ShellRoute::Settings(SettingsCategory::ComputerUse)
                ) =>
            {
                AppCommand::SetComputerAllowedAppInput(value)
            }
            _ => return,
        };
        // `SetComputerAllowedAppInput` needs real I/O-free state mutation
        // that `reduce_settings_command` (the pure `zode-app-model` reducer)
        // cannot own - see `computer_use::consume_computer_use_command`.
        // Every other command here falls through unchanged.
        if super::computer_use::consume_computer_use_command(
            &mut self.app_state,
            self.external_open.as_ref(),
            &command,
        ) {
            self.rebuild_frame_snapshot();
            self.request_redraw();
            return;
        }
        if reduce_settings_command(&mut self.app_state, command) == SettingsCommandOutcome::Applied
        {
            self.rebuild_frame_snapshot();
            self.request_redraw();
        }
    }

    fn project_provider_input_value(&mut self, id: WidgetId, value: String) {
        if id == PROVIDER_API_KEY_INPUT_ID {
            let kind = self
                .app_state
                .provider_models
                .editor
                .as_ref()
                .map(|draft| draft.kind)
                .unwrap_or_default();
            self.app_state.provider_models.credential_configured = self
                .provider_models_controller
                .credential_configured_for(kind);
            self.app_state.provider_models.pending_secret_len =
                self.provider_models_controller.pending_secret_len();
            self.rebuild_frame_snapshot();
            self.request_redraw();
            return;
        }
        let Some(mut draft) = self.app_state.provider_models.editor.clone() else {
            return;
        };
        match id {
            PROVIDER_ID_INPUT_ID => draft.provider_id = value,
            PROVIDER_BASE_URL_INPUT_ID => draft.base_url = Some(value),
            PROVIDER_MODEL_IDS_INPUT_ID => draft.model_ids = parse_model_ids(&value),
            _ => return,
        }
        if reduce_settings_command(&mut self.app_state, AppCommand::UpdateProviderDraft(draft))
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
            self.invalidate_frame_snapshot_for_scroll();
            self.request_redraw();
        }
    }
}

pub(super) const fn is_provider_models_input(id: WidgetId) -> bool {
    matches!(
        id,
        PROVIDER_ID_INPUT_ID
            | PROVIDER_BASE_URL_INPUT_ID
            | PROVIDER_API_KEY_INPUT_ID
            | PROVIDER_MODEL_IDS_INPUT_ID
    )
}

fn parse_model_ids(value: &str) -> Vec<String> {
    value
        .split([',', '\n', '\r'])
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .fold(Vec::new(), |mut models, model| {
            if !models.iter().any(|existing| existing == model) {
                models.push(model.to_owned());
            }
            models
        })
}

fn edit_search_query(query: &mut String, event: &KeyEvent) -> SearchKeyOutcome {
    match &event.key {
        Key::Escape if !query.is_empty() => query.clear(),
        Key::Escape => return SearchKeyOutcome::Ignored,
        Key::Backspace => {
            query.pop();
        }
        Key::Delete => query.clear(),
        Key::Character(value) if event.modifiers.primary() && value.eq_ignore_ascii_case("a") => {
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
        | Key::PageDown => return SearchKeyOutcome::Consumed,
        Key::Tab | Key::Character(_) => return SearchKeyOutcome::Ignored,
    }
    SearchKeyOutcome::Changed
}

fn append_ime_commit(query: &mut String, event: &zode_app_ui::ImeEvent) -> bool {
    let zode_app_ui::ImeEvent::Commit(text) = event else {
        return false;
    };
    query.push_str(text);
    true
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
    use super::{
        append_ime_commit, edit_search_query, parse_model_ids, parse_worktrees, SearchKeyOutcome,
    };
    use zode_app_ui::{ImeEvent, Key, KeyEvent, Modifiers};

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

    #[test]
    fn settings_search_keyboard_and_ime_edits_are_unicode_safe() {
        let mut query = "归档".to_owned();
        assert_eq!(
            edit_search_query(
                &mut query,
                &KeyEvent {
                    key: Key::Backspace,
                    modifiers: Modifiers::NONE,
                    pressed: true,
                },
            ),
            SearchKeyOutcome::Changed,
        );
        assert_eq!(query, "归");
        assert!(append_ime_commit(
            &mut query,
            &ImeEvent::Commit("任务".into())
        ));
        assert_eq!(query, "归任务");
        assert!(!append_ime_commit(
            &mut query,
            &ImeEvent::Update {
                text: "候选".into(),
                cursor: Some(2),
            },
        ));
        assert_eq!(query, "归任务");
    }

    #[test]
    fn provider_model_input_preserves_order_and_removes_duplicates() {
        assert_eq!(
            parse_model_ids("gpt-5.6, claude-opus\ngpt-5.6\r\nqwen3"),
            vec!["gpt-5.6", "claude-opus", "qwen3"]
        );
    }
}
