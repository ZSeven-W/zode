use zode_app_model::{
    AppCommand, BranchCatalogState, ComposerContextMenu, NavigationOutcome, ZodeAppState,
};
use zode_app_ui::{
    ComposerContextMenu as ComposerContextMenuWidget, ImeEvent, Key, KeyEvent, Modifiers,
    ProjectSearchOutcome, WidgetId, WorkspaceSnapshot, COMPOSER_BRANCH_ID,
    COMPOSER_BRANCH_SEARCH_ID, COMPOSER_CONTEXT_MENU_SURFACE_ID, COMPOSER_LOCATION_ID,
    COMPOSER_LOCATION_LOCAL_ID, COMPOSER_PROJECT_ID, PROJECT_DETACH_ID,
};

use super::DesktopApp;
use crate::clipboard::ClipboardService;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComposerContextPointerOutcome {
    Ignored,
    Actionable,
    Captured,
    Close,
}

fn composer_context_pointer_outcome(
    state: &ZodeAppState,
    snapshot: &WorkspaceSnapshot,
    point: jian_widgets::Point2D,
) -> ComposerContextPointerOutcome {
    if state.composer.context_menu.is_none() {
        return ComposerContextPointerOutcome::Ignored;
    }
    let hit = snapshot.hit_test(point);
    let Some(surface) = snapshot
        .node(COMPOSER_CONTEXT_MENU_SURFACE_ID)
        .map(|node| node.rect)
    else {
        return ComposerContextPointerOutcome::Close;
    };
    if surface.contains(point) {
        return if hit.is_some_and(|id| composer_context_menu_actionable_widget(state, id)) {
            ComposerContextPointerOutcome::Actionable
        } else {
            ComposerContextPointerOutcome::Captured
        };
    }
    if hit.is_some_and(is_composer_context_trigger) {
        ComposerContextPointerOutcome::Actionable
    } else {
        ComposerContextPointerOutcome::Close
    }
}

fn composer_context_menu_actionable_widget(state: &ZodeAppState, id: WidgetId) -> bool {
    (id == COMPOSER_BRANCH_SEARCH_ID
        && state.composer.context_menu == Some(ComposerContextMenu::Branch))
        || ComposerContextMenuWidget::command_for_widget(state, id).is_some()
}

fn is_composer_context_trigger(id: WidgetId) -> bool {
    matches!(
        id,
        COMPOSER_PROJECT_ID | PROJECT_DETACH_ID | COMPOSER_LOCATION_ID | COMPOSER_BRANCH_ID
    )
}

impl DesktopApp {
    pub(super) fn handle_composer_context_menu_pointer(
        &mut self,
        point: jian_widgets::Point2D,
    ) -> bool {
        match composer_context_pointer_outcome(&self.app_state, &self.frame_snapshot, point) {
            ComposerContextPointerOutcome::Ignored | ComposerContextPointerOutcome::Actionable => {
                false
            }
            ComposerContextPointerOutcome::Captured => true,
            ComposerContextPointerOutcome::Close => {
                self.enqueue_command(AppCommand::CloseComposerContextMenu);
                true
            }
        }
    }

    pub(super) fn composer_context_allows_accessibility_action(&self, id: WidgetId) -> bool {
        if self.app_state.composer.context_menu.is_none() {
            return true;
        }
        self.frame_snapshot.node(id).is_some_and(|node| {
            !node.disabled
                && !node.actions.is_empty()
                && composer_context_menu_actionable_widget(&self.app_state, id)
        })
    }

    pub(super) fn branch_load_after_context_toggle(
        &self,
        command: &AppCommand,
    ) -> Option<zode_node_protocol::WorkspaceUri> {
        if !matches!(
            command,
            AppCommand::ToggleComposerContextMenu(ComposerContextMenu::Branch)
        ) || self.app_state.composer.context_menu != Some(ComposerContextMenu::Branch)
        {
            return None;
        }
        self.app_state
            .active_available_workspace()
            .cloned()
            .filter(|workspace| branch_catalog_needs_load(&self.app_state, workspace))
    }

    pub(super) fn consume_branch_navigation_outcome(
        &mut self,
        command: &AppCommand,
        outcome: NavigationOutcome,
    ) -> Option<bool> {
        if outcome == NavigationOutcome::NeedsEffect {
            match command {
                AppCommand::LoadBranches { workspace_uri } => {
                    self.request_branch_catalog(workspace_uri.clone());
                    return Some(true);
                }
                AppCommand::SelectBranch {
                    workspace_uri,
                    branch,
                } => {
                    let expected_current = match &self.app_state.composer.branch_picker.catalog {
                        BranchCatalogState::Switching {
                            workspace_uri: switching,
                            from,
                            branch: switching_branch,
                        } if switching == workspace_uri && switching_branch == branch => {
                            from.clone()
                        }
                        _ => return Some(true),
                    };
                    self.request_branch_switch(
                        workspace_uri.clone(),
                        expected_current,
                        branch.clone(),
                    );
                    return Some(true);
                }
                _ => {}
            }
        }
        (outcome == NavigationOutcome::Ignored
            && matches!(
                command,
                AppCommand::LoadBranches { .. }
                    | AppCommand::BranchesLoaded(_)
                    | AppCommand::BranchesFailed { .. }
                    | AppCommand::SelectBranch { .. }
            ))
        .then_some(true)
    }

    pub(super) fn sync_composer_context_after_navigation(
        &mut self,
        command: &AppCommand,
        previous_menu: Option<ComposerContextMenu>,
    ) -> Option<WidgetId> {
        match command {
            AppCommand::ToggleComposerContextMenu(ComposerContextMenu::Branch) => {
                if self.app_state.composer.context_menu == Some(ComposerContextMenu::Branch) {
                    self.branch_picker_controller
                        .set_text(self.app_state.composer.branch_picker.query.clone());
                    Some(COMPOSER_BRANCH_SEARCH_ID)
                } else {
                    Some(COMPOSER_BRANCH_ID)
                }
            }
            AppCommand::ToggleComposerContextMenu(ComposerContextMenu::Location) => {
                if self.app_state.composer.context_menu == Some(ComposerContextMenu::Location) {
                    Some(COMPOSER_LOCATION_LOCAL_ID)
                } else {
                    Some(COMPOSER_LOCATION_ID)
                }
            }
            AppCommand::CloseComposerContextMenu => previous_menu.map(context_menu_trigger),
            AppCommand::SelectTaskLaunchMode(_) => Some(COMPOSER_LOCATION_ID),
            AppCommand::SelectBranch { .. } => Some(COMPOSER_BRANCH_ID),
            _ => None,
        }
    }

    pub(super) fn handle_branch_picker_ime(&mut self, event: ImeEvent) -> bool {
        if self.app_state.composer.context_menu != Some(ComposerContextMenu::Branch)
            || self.focused_widget != Some(COMPOSER_BRANCH_SEARCH_ID)
        {
            return false;
        }
        if self.branch_picker_controller.ime(event) == ProjectSearchOutcome::Edited {
            self.sync_branch_search_from_controller();
        }
        true
    }

    pub(super) fn handle_branch_picker_key(&mut self, event: &KeyEvent) -> bool {
        let Some(menu) = self.app_state.composer.context_menu else {
            return false;
        };
        if !event.pressed {
            return false;
        }
        if event.key == Key::Escape {
            self.enqueue_command(AppCommand::CloseComposerContextMenu);
            return true;
        }
        let focus_ids = self.composer_context_focus_ids();
        if event.key == Key::Tab {
            self.cycle_composer_context_focus(
                &focus_ids,
                event.modifiers.contains(Modifiers::SHIFT),
            );
            return true;
        }
        if matches!(event.key, Key::ArrowUp | Key::ArrowDown) {
            self.cycle_composer_context_focus(&focus_ids, event.key == Key::ArrowUp);
            return true;
        }
        if menu == ComposerContextMenu::Location {
            return false;
        }
        if self.focused_widget != Some(COMPOSER_BRANCH_SEARCH_ID) {
            return false;
        }
        if event.key == Key::Enter
            && self
                .branch_picker_controller
                .input_state()
                .composition()
                .is_some()
        {
            let _ = self
                .branch_picker_controller
                .key(event.key.clone(), event.modifiers);
            self.sync_branch_search_from_controller();
            return true;
        }
        if self
            .branch_picker_controller
            .key(event.key.clone(), event.modifiers)
            == ProjectSearchOutcome::Edited
        {
            self.sync_branch_search_from_controller();
            return true;
        }
        if event.key == Key::Enter {
            if let Some(id) = focus_ids.get(1).copied() {
                self.activate_widget(id);
            }
            return true;
        }
        let is_paste = event.modifiers.primary()
            && matches!(&event.key, Key::Character(value) if value.eq_ignore_ascii_case("v"));
        !is_paste && event.modifiers.primary()
    }

    pub(super) fn set_branch_search_value(&mut self, value: String) {
        self.branch_picker_controller.set_text(value);
        self.sync_branch_search_from_controller();
    }

    pub(super) fn paste_branch_search_text(&mut self, text: &str) -> bool {
        if self.app_state.composer.context_menu != Some(ComposerContextMenu::Branch)
            || self.focused_widget != Some(COMPOSER_BRANCH_SEARCH_ID)
        {
            return false;
        }
        if self.branch_picker_controller.paste_text(text) == ProjectSearchOutcome::Edited {
            self.sync_branch_search_from_controller();
        }
        true
    }

    pub(super) fn paste_branch_search_from_clipboard(
        &mut self,
        clipboard: &dyn ClipboardService,
    ) -> bool {
        if self.app_state.composer.context_menu != Some(ComposerContextMenu::Branch)
            || self.focused_widget != Some(COMPOSER_BRANCH_SEARCH_ID)
        {
            return false;
        }
        match clipboard.read_text() {
            Ok(Some(text)) if !text.is_empty() => {
                let _ = self.paste_branch_search_text(&text);
            }
            Ok(_) => {}
            Err(error) => eprintln!("zode-app: clipboard read failed: {error}"),
        }
        true
    }

    fn sync_branch_search_from_controller(&mut self) {
        self.enqueue_command(AppCommand::SetBranchSearch(
            self.branch_picker_controller.text().to_owned(),
        ));
    }

    fn composer_context_focus_ids(&self) -> Vec<WidgetId> {
        let Some(surface) = self
            .frame_snapshot
            .node(COMPOSER_CONTEXT_MENU_SURFACE_ID)
            .map(|node| node.rect)
        else {
            return Vec::new();
        };
        self.frame_snapshot
            .focusable_ids()
            .into_iter()
            .filter(|id| {
                self.frame_snapshot.node(*id).is_some_and(|node| {
                    let center = jian_widgets::Point2D::new(
                        node.rect.origin.x + node.rect.size.x / 2.0,
                        node.rect.origin.y + node.rect.size.y / 2.0,
                    );
                    surface.contains(center)
                })
            })
            .collect()
    }

    fn cycle_composer_context_focus(&mut self, ids: &[WidgetId], backwards: bool) {
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
        self.set_focused_widget(Some(ids[index]));
    }
}

fn branch_catalog_needs_load(
    state: &ZodeAppState,
    workspace_uri: &zode_node_protocol::WorkspaceUri,
) -> bool {
    match &state.composer.branch_picker.catalog {
        BranchCatalogState::Ready(_) => true,
        BranchCatalogState::Loading {
            workspace_uri: loading,
        } => loading != workspace_uri,
        BranchCatalogState::Switching {
            workspace_uri: switching,
            ..
        } => switching != workspace_uri,
        BranchCatalogState::Idle | BranchCatalogState::Failed { .. } => true,
    }
}

fn context_menu_trigger(menu: ComposerContextMenu) -> WidgetId {
    match menu {
        ComposerContextMenu::Location => COMPOSER_LOCATION_ID,
        ComposerContextMenu::Branch => COMPOSER_BRANCH_ID,
    }
}

#[cfg(test)]
mod tests {
    use jian_widgets::{Point2D, Rect};
    use zode_app_model::{
        demo_state, BranchCatalog, BranchCatalogState, ComposerContextMenu, ProjectState,
    };
    use zode_app_ui::{
        Insets, WorkspaceSnapshot, COMPOSER_BRANCH_ID, COMPOSER_CONTEXT_MENU_SURFACE_ID,
        COMPOSER_LOCATION_LOCAL_ID, COMPOSER_LOCATION_WORKTREE_ID,
    };
    use zode_node_protocol::WorkspaceUri;

    use super::{composer_context_pointer_outcome, ComposerContextPointerOutcome};

    fn center(rect: Rect) -> Point2D {
        Point2D::new(
            rect.origin.x + rect.size.x / 2.0,
            rect.origin.y + rect.size.y / 2.0,
        )
    }

    fn location_menu_state() -> zode_app_model::ZodeAppState {
        let mut state = demo_state();
        let workspace_uri = WorkspaceUri::new("file:///tmp/zode-modal-menu").unwrap();
        state.current_session = None;
        state.projects = vec![ProjectState {
            workspace_uri: workspace_uri.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 0,
        }];
        state.active_workspace = Some(workspace_uri.clone());
        state.composer.context_menu = Some(ComposerContextMenu::Location);
        state.composer.branch_picker.catalog = BranchCatalogState::Ready(BranchCatalog {
            workspace_uri,
            current: "main".into(),
            branches: vec!["main".into(), "feature/modal".into()],
            dirty_files: 0,
        });
        state
    }

    #[test]
    fn menu_surface_captures_blank_and_disabled_regions_but_allows_rows() {
        let state = location_menu_state();
        let snapshot = WorkspaceSnapshot::build(&state, 1_200.0, 900.0, Insets::ZERO);
        let surface = snapshot
            .node(COMPOSER_CONTEXT_MENU_SURFACE_ID)
            .expect("context menu surface")
            .rect;
        let disabled = snapshot
            .node(COMPOSER_LOCATION_WORKTREE_ID)
            .expect("disabled worktree row")
            .rect;
        let enabled = snapshot
            .node(COMPOSER_LOCATION_LOCAL_ID)
            .expect("enabled local row")
            .rect;

        assert_eq!(
            composer_context_pointer_outcome(
                &state,
                &snapshot,
                Point2D::new(surface.origin.x + 12.0, surface.origin.y + 12.0),
            ),
            ComposerContextPointerOutcome::Captured
        );
        assert_eq!(
            composer_context_pointer_outcome(&state, &snapshot, center(disabled)),
            ComposerContextPointerOutcome::Captured
        );
        assert_eq!(
            composer_context_pointer_outcome(&state, &snapshot, center(enabled)),
            ComposerContextPointerOutcome::Actionable
        );
    }

    #[test]
    fn another_composer_trigger_stays_actionable_while_menu_is_open() {
        let state = location_menu_state();
        let snapshot = WorkspaceSnapshot::build(&state, 1_200.0, 900.0, Insets::ZERO);
        let branch = snapshot
            .node(COMPOSER_BRANCH_ID)
            .expect("branch trigger")
            .rect;

        assert_eq!(snapshot.hit_test(center(branch)), Some(COMPOSER_BRANCH_ID));
        assert_eq!(
            composer_context_pointer_outcome(&state, &snapshot, center(branch)),
            ComposerContextPointerOutcome::Actionable
        );
        assert_eq!(
            composer_context_pointer_outcome(&state, &snapshot, Point2D::new(900.0, 500.0)),
            ComposerContextPointerOutcome::Close
        );
    }
}
