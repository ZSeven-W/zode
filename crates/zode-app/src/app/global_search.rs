use zode_app_model::AppCommand;
use zode_app_ui::{
    GlobalSearch, GlobalSearchOutcome, ImeEvent, Key, KeyEvent, Modifiers, WidgetId,
    GLOBAL_SEARCH_INPUT_ID, GLOBAL_SEARCH_NEW_TASK_ID, GLOBAL_SEARCH_OPEN_FOLDER_ID,
    GLOBAL_SEARCH_SCRIM_ID, GLOBAL_SEARCH_SETTINGS_ID, GLOBAL_SEARCH_SURFACE_ID, SIDEBAR_SEARCH_ID,
};

use super::DesktopApp;

impl DesktopApp {
    pub(super) fn global_search_view_state(&self) -> zode_app_ui::GlobalSearchViewState {
        zode_app_ui::GlobalSearchViewState {
            open: self.app_state.global_search.open,
            query: self.global_search_controller.text().to_owned(),
            active_index: self.app_state.global_search.active_index,
        }
    }

    pub(super) fn sync_global_search_after_navigation(
        &mut self,
        command: &AppCommand,
        was_open: bool,
    ) -> Option<WidgetId> {
        if !self.app_state.global_search.open {
            self.global_search_controller.set_text("");
        }
        match command {
            AppCommand::ToggleGlobalSearch if self.app_state.global_search.open && !was_open => {
                self.global_search_controller.set_text("");
                Some(GLOBAL_SEARCH_INPUT_ID)
            }
            AppCommand::ToggleGlobalSearch | AppCommand::CloseGlobalSearch => {
                Some(SIDEBAR_SEARCH_ID)
            }
            AppCommand::SelectSession(_)
            | AppCommand::BeginTask { .. }
            | AppCommand::CreateProject
            | AppCommand::Navigate(_) => None,
            _ => None,
        }
    }

    pub(super) fn global_search_contains_point(&self, point: jian_widgets::Point2D) -> bool {
        self.frame_snapshot
            .node(GLOBAL_SEARCH_SURFACE_ID)
            .is_some_and(|node| node.rect.contains(point))
    }

    pub(super) fn global_search_allows_accessibility_action(&self, id: WidgetId) -> bool {
        if !self.app_state.global_search.open {
            return true;
        }
        matches!(
            id,
            GLOBAL_SEARCH_INPUT_ID
                | GLOBAL_SEARCH_NEW_TASK_ID
                | GLOBAL_SEARCH_OPEN_FOLDER_ID
                | GLOBAL_SEARCH_SETTINGS_ID
                | GLOBAL_SEARCH_SCRIM_ID
                | GLOBAL_SEARCH_SURFACE_ID
                | SIDEBAR_SEARCH_ID
        ) || GlobalSearch::choices(&self.app_state, self.global_search_controller.text())
            .into_iter()
            .any(|choice| choice.id == id)
    }

    pub(super) fn close_global_search_from_outside(&mut self) {
        self.enqueue_command(AppCommand::CloseGlobalSearch);
    }

    pub(super) fn handle_global_search_ime(&mut self, event: ImeEvent) -> bool {
        if !self.app_state.global_search.open || self.focused_widget != Some(GLOBAL_SEARCH_INPUT_ID)
        {
            return false;
        }
        if self.global_search_controller.ime(event) == GlobalSearchOutcome::Edited {
            self.sync_global_search_from_controller();
        }
        true
    }

    pub(super) fn handle_global_search_key(&mut self, event: &KeyEvent) -> bool {
        if !self.app_state.global_search.open || !event.pressed {
            return false;
        }
        if event.key == Key::Escape {
            self.enqueue_command(AppCommand::CloseGlobalSearch);
            return true;
        }
        let focus_ids = self.global_search_focus_ids();
        if event.key == Key::Tab {
            self.cycle_global_search_focus(&focus_ids, event.modifiers.contains(Modifiers::SHIFT));
            return true;
        }
        if matches!(event.key, Key::ArrowUp | Key::ArrowDown) {
            let item_count = focus_ids.len().saturating_sub(1);
            if let Some(next) = next_global_search_active(
                self.app_state.global_search.active_index,
                item_count,
                event.key == Key::ArrowUp,
            ) {
                self.enqueue_command(AppCommand::SetGlobalSearchActive(next));
                self.set_focused_widget(Some(GLOBAL_SEARCH_INPUT_ID));
            }
            return true;
        }
        if self.focused_widget == Some(GLOBAL_SEARCH_INPUT_ID) {
            if event.key == Key::Enter
                && self
                    .global_search_controller
                    .input_state()
                    .composition()
                    .is_some()
            {
                let _ = self
                    .global_search_controller
                    .key(event.key.clone(), event.modifiers);
                self.sync_global_search_from_controller();
                return true;
            }
            if event.key == Key::Enter {
                let view = self.global_search_view_state();
                if let Some(layout) = GlobalSearch::layout(
                    self.frame_snapshot.layout.viewport,
                    &self.app_state,
                    &view,
                ) {
                    if let Some(command) =
                        GlobalSearch::command_for_active(&self.app_state, &view, &layout)
                    {
                        self.enqueue_command(command);
                    }
                }
                return true;
            }
            if self
                .global_search_controller
                .key(event.key.clone(), event.modifiers)
                == GlobalSearchOutcome::Edited
            {
                self.sync_global_search_from_controller();
                return true;
            }
            let is_paste = event.modifiers.primary()
                && matches!(&event.key, Key::Character(value) if value.eq_ignore_ascii_case("v"));
            return !is_paste;
        }
        event.modifiers.primary()
    }

    pub(super) fn set_global_search_value(&mut self, value: String) {
        self.global_search_controller.set_text(value);
        self.sync_global_search_from_controller();
    }

    pub(super) fn paste_global_search_text(&mut self, text: &str) -> bool {
        if !self.app_state.global_search.open || self.focused_widget != Some(GLOBAL_SEARCH_INPUT_ID)
        {
            return false;
        }
        if self.global_search_controller.paste_text(text) == GlobalSearchOutcome::Edited {
            self.sync_global_search_from_controller();
        }
        true
    }

    fn sync_global_search_from_controller(&mut self) {
        self.enqueue_command(AppCommand::SetGlobalSearchQuery(
            self.global_search_controller.text().to_owned(),
        ));
    }

    fn global_search_focus_ids(&self) -> Vec<WidgetId> {
        let mut ids = vec![GLOBAL_SEARCH_INPUT_ID];
        let view = self.global_search_view_state();
        if let Some(layout) =
            GlobalSearch::layout(self.frame_snapshot.layout.viewport, &self.app_state, &view)
        {
            ids.extend(GlobalSearch::selectable_ids(&layout));
        }
        ids.retain(|id| self.frame_snapshot.node(*id).is_some());
        ids
    }

    fn cycle_global_search_focus(&mut self, ids: &[WidgetId], backwards: bool) {
        if ids.is_empty() {
            return;
        }
        let current = self
            .focused_widget
            .and_then(|focused| ids.iter().position(|id| *id == focused));
        let index = match (backwards, current) {
            (false, Some(index)) => (index + 1) % ids.len(),
            (true, Some(0)) => ids.len() - 1,
            (true, Some(index)) => index - 1,
            (false, None) => 0,
            (true, None) => ids.len() - 1,
        };
        self.enqueue_command(AppCommand::SetGlobalSearchActive(index.saturating_sub(1)));
        self.set_focused_widget(Some(ids[index]));
    }
}

fn next_global_search_active(
    active_index: usize,
    selectable_count: usize,
    backwards: bool,
) -> Option<usize> {
    let last = selectable_count.checked_sub(1)?;
    let current = active_index.min(last);
    Some(if backwards {
        current.checked_sub(1).unwrap_or(last)
    } else {
        (current + 1) % selectable_count
    })
}

#[cfg(test)]
mod tests {
    use jian_widgets::Rect;
    use zode_app_model::{demo_state, AppCommand, SettingsCategory, ShellRoute};
    use zode_app_ui::{GlobalSearch, GlobalSearchViewState};
    use zode_node_protocol::{SessionLocator, ThreadStatus, ThreadSummary, WorkspaceUri};

    use super::next_global_search_active;

    #[test]
    fn narrow_layout_arrow_selection_and_enter_command_use_the_same_visible_order() {
        let mut state = demo_state();
        state.threads.clear();
        let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
        for index in 0..8 {
            state.threads.push(ThreadSummary {
                session: SessionLocator::new(state.host.node_id, format!("task-{index}")),
                workspace_uri: workspace.clone(),
                title: format!("Task {index}"),
                updated_at_ms: index,
                status: ThreadStatus::Idle,
            });
        }
        let viewport = Rect::xywh(0.0, 0.0, 300.0, 240.0);
        let mut view = GlobalSearchViewState {
            open: true,
            query: String::new(),
            active_index: 0,
        };
        let layout = GlobalSearch::layout(viewport, &state, &view).unwrap();
        let selectable_count = GlobalSearch::selectable_ids(&layout).len();
        assert_eq!(selectable_count, 4, "one visible task plus three actions");

        view.active_index = next_global_search_active(0, selectable_count, false).unwrap();
        let layout = GlobalSearch::layout(viewport, &state, &view).unwrap();
        assert!(layout.action_rows[0].active);
        assert!(matches!(
            GlobalSearch::command_for_active(&state, &view, &layout),
            Some(AppCommand::BeginTask { .. })
        ));

        view.active_index = next_global_search_active(0, selectable_count, true).unwrap();
        let layout = GlobalSearch::layout(viewport, &state, &view).unwrap();
        assert!(layout.action_rows[2].active);
        assert_eq!(
            GlobalSearch::command_for_active(&state, &view, &layout),
            Some(AppCommand::Navigate(ShellRoute::Settings(
                SettingsCategory::General,
            )))
        );
    }
}
