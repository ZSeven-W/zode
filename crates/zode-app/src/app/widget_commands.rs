use zode_app_model::{
    AppCommand, ComposerContextMenu, ComposerFooterMenu, ThemePreference, ZodeAppState,
};
use zode_app_ui::{
    Composer, ComposerContextMenu as ComposerContextMenuWidget, ComposerFooterMenuWidget,
    DocumentPreview, EnvironmentPanel, GlobalSearch, IntegrationsPage, Lightbox, PinnedSummaryMode,
    ProjectPicker, ProjectSidebar, ReviewPanel, SettingsPanel, SidebarAction, SubagentsPanel,
    TerminalSecondaryPanel, ThreadHeader, ThreadTranscript, UnavailableSecondaryPanel, WidgetId,
    WorkspaceSnapshot, COMPOSER_ADD_ID, COMPOSER_BRANCH_ID, COMPOSER_LOCATION_ID,
    COMPOSER_MODEL_ID, COMPOSER_PERMISSION_ID, COMPOSER_PROJECT_ID, ENVIRONMENT_CLOSE_ID,
    HEADER_ENVIRONMENT_ID, HIGH_CONTRAST_ID, PROJECT_PICKER_NEW_ID, PROJECT_PICKER_PROJECTLESS_ID,
    PROJECT_PICKER_TRIGGER_ID, REDUCED_MOTION_ID, SECONDARY_PANE_BREAKPOINT, THEME_DARK_ID,
    THEME_LIGHT_ID, THEME_SYSTEM_ID,
};

pub(super) fn widget_command_for_snapshot(
    state: &ZodeAppState,
    snapshot: &WorkspaceSnapshot,
    id: WidgetId,
) -> Option<AppCommand> {
    if id == zode_app_ui::TRANSCRIPT_BACK_TO_BOTTOM_ID {
        // Needs the transcript viewport rect and tool-expanded overrides, so
        // it bypasses `ThreadTranscript::command_for_widget` (matched purely
        // by widget id, like the rest of `widget_command`) entirely.
        let empty = std::collections::BTreeMap::new();
        let tool_expanded = state
            .current_session
            .as_ref()
            .and_then(|session| state.tool_expanded.get(session))
            .unwrap_or(&empty);
        return ThreadTranscript::back_to_bottom_command(
            state,
            snapshot.layout.transcript,
            tool_expanded,
        );
    }
    // Anchor-rail ticks: same reasoning as the back-to-bottom button above -
    // resolving a click needs the transcript viewport and the outer
    // primary-surface rect (for the same hide-rule geometry the rail itself
    // paints and registers a11y nodes with), which the generic
    // `widget_command` id-only dispatch below has no access to. Harmless
    // (and cheap - bounded by turn count) to try unconditionally: it just
    // returns `None` for any id that isn't one of the rail's own ticks.
    if let Some(command) = anchor_rail_command(state, snapshot, id) {
        return Some(command);
    }
    if matches!(id, HEADER_ENVIRONMENT_ID | ENVIRONMENT_CLOSE_ID) && state.current_session.is_some()
    {
        return match snapshot.layout.pinned_summary {
            PinnedSummaryMode::Docked => Some(AppCommand::SetPinnedSummaryAutoHidden(true)),
            PinnedSummaryMode::Overlay => Some(AppCommand::SetPinnedSummaryOverlayOpen(false)),
            PinnedSummaryMode::Hidden if snapshot.layout.viewport.size.x > 0.0 => {
                if !state.presentation.secondary_sidebar_open
                    && snapshot.layout.viewport.size.x >= SECONDARY_PANE_BREAKPOINT
                    && state.presentation.pinned_summary_auto_hidden
                {
                    Some(AppCommand::SetPinnedSummaryAutoHidden(false))
                } else {
                    Some(AppCommand::SetPinnedSummaryOverlayOpen(true))
                }
            }
            PinnedSummaryMode::Hidden => None,
        };
    }
    widget_command(state, id)
}

pub(super) fn widget_command(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    static_sidebar_command(state, id)
        .or_else(|| Lightbox::command_for_widget(state, id))
        .or_else(|| ProjectSidebar::command_for_widget(state, id))
        .or_else(|| ThreadHeader::command_for_widget(state, id))
        .or_else(|| IntegrationsPage::command_for_widget(state, id))
        .or_else(|| SettingsPanel::command_for_widget(state, id))
        .or_else(|| EnvironmentPanel::command_for_widget(state, id))
        .or_else(|| ReviewPanel::command_for_widget(state, id))
        .or_else(|| DocumentPreview::command_for_widget(state, id))
        .or_else(|| TerminalSecondaryPanel::command_for_widget(state, id))
        .or_else(|| UnavailableSecondaryPanel::command_for_widget(state, id))
        .or_else(|| SubagentsPanel::command_for_widget(state, id))
        .or_else(|| appearance_command(state, id))
        .or_else(|| GlobalSearch::command_for_widget(state, id))
        .or_else(|| project_picker_command(state, id))
        .or_else(|| composer_context_command(state, id))
        .or_else(|| ComposerContextMenuWidget::command_for_widget(state, id))
        .or_else(|| composer_footer_command(id))
        .or_else(|| ComposerFooterMenuWidget::command_for_widget(state, id))
        .or_else(|| Composer::command_for_widget(state, id))
        .or_else(|| ThreadTranscript::command_for_widget(state, id))
}

fn anchor_rail_command(
    state: &ZodeAppState,
    snapshot: &WorkspaceSnapshot,
    id: WidgetId,
) -> Option<AppCommand> {
    let empty = std::collections::BTreeMap::new();
    let tool_expanded = state
        .current_session
        .as_ref()
        .and_then(|session| state.tool_expanded.get(session))
        .unwrap_or(&empty);
    zode_app_ui::AnchorRail::command_for_widget(
        state,
        snapshot.layout.transcript,
        snapshot.layout.primary_surface,
        tool_expanded,
        id,
    )
}

fn composer_footer_command(id: WidgetId) -> Option<AppCommand> {
    match id {
        COMPOSER_ADD_ID => Some(AppCommand::ToggleComposerFooterMenu(
            ComposerFooterMenu::Add,
        )),
        COMPOSER_PERMISSION_ID => Some(AppCommand::ToggleComposerFooterMenu(
            ComposerFooterMenu::Permission,
        )),
        COMPOSER_MODEL_ID => Some(AppCommand::ToggleComposerFooterMenu(
            ComposerFooterMenu::Model,
        )),
        _ => None,
    }
}

fn project_picker_command(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    if id == PROJECT_PICKER_TRIGGER_ID && state.current_session.is_none() {
        return Some(AppCommand::ToggleProjectPicker);
    }
    if id == COMPOSER_PROJECT_ID && state.current_session.is_none() {
        return Some(AppCommand::ToggleComposerProjectPicker);
    }
    if id == PROJECT_PICKER_NEW_ID && state.project_picker.open {
        return Some(AppCommand::CreateProject);
    }
    if id == PROJECT_PICKER_PROJECTLESS_ID && state.project_picker.open {
        return Some(AppCommand::BeginTask {
            workspace_uri: None,
        });
    }
    if !state.project_picker.open {
        return None;
    }
    ProjectPicker::choices(state, &state.project_picker.search)
        .into_iter()
        .find(|choice| ProjectPicker::project_widget_id(&choice.workspace_uri) == id)
        .map(|choice| AppCommand::BeginTask {
            workspace_uri: Some(choice.workspace_uri),
        })
}

fn composer_context_command(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    if state.current_session.is_some() {
        return None;
    }
    match id {
        COMPOSER_LOCATION_ID => Some(AppCommand::ToggleComposerContextMenu(
            ComposerContextMenu::Location,
        )),
        COMPOSER_BRANCH_ID if state.active_available_workspace().is_some() => Some(
            AppCommand::ToggleComposerContextMenu(ComposerContextMenu::Branch),
        ),
        _ => None,
    }
}

fn static_sidebar_command(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    let action = match id.0 {
        2..=7 => {
            ProjectSidebar::navigation_items()
                .get((id.0 - 2) as usize)?
                .action
        }
        9 => ProjectSidebar::footer_item().action,
        10 => SidebarAction::Navigate(zode_app_model::ShellRoute::ComingSoon(
            zode_app_model::ComingSoonFeature::Help,
        )),
        _ => return None,
    };
    match action {
        SidebarAction::NewSession => new_session_command(state),
        SidebarAction::Navigate(route) => Some(AppCommand::Navigate(route)),
    }
}

fn new_session_command(state: &ZodeAppState) -> Option<AppCommand> {
    Some(AppCommand::BeginTask {
        workspace_uri: state.active_available_workspace().cloned(),
    })
}

fn appearance_command(state: &ZodeAppState, id: WidgetId) -> Option<AppCommand> {
    match id {
        THEME_SYSTEM_ID => Some(AppCommand::SetThemePreference(ThemePreference::System)),
        THEME_LIGHT_ID => Some(AppCommand::SetThemePreference(ThemePreference::Light)),
        THEME_DARK_ID => Some(AppCommand::SetThemePreference(ThemePreference::Dark)),
        REDUCED_MOTION_ID => Some(AppCommand::SetReducedMotion(
            !state.ui_preferences.reduced_motion,
        )),
        HIGH_CONTRAST_ID => Some(AppCommand::SetHighContrast(
            !state.ui_preferences.high_contrast,
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use jian_widgets::Point2D;
    use zode_app_model::{
        demo_state, reduce_navigation_command, AppCommand, ComposerFooterMenu, NavigationOutcome,
        ProjectState,
    };
    use zode_app_ui::{
        GlobalSearch, GlobalSearchViewState, Insets, ProjectPicker, WidgetId, WorkspaceSnapshot,
        COMPOSER_ADD_ID, COMPOSER_BRANCH_ID, COMPOSER_CONTEXT_MENU_SURFACE_ID,
        COMPOSER_FOOTER_MENU_SURFACE_ID, COMPOSER_LOCATION_ID, COMPOSER_MODEL_ID,
        COMPOSER_PERMISSION_ID, COMPOSER_PROJECT_ID, GLOBAL_SEARCH_INPUT_ID,
        GLOBAL_SEARCH_SURFACE_ID, PROJECT_DETACH_ID, PROJECT_PICKER_NEW_ID,
        PROJECT_PICKER_PROJECTLESS_ID, PROJECT_PICKER_SEARCH_ID, PROJECT_PICKER_SURFACE_ID,
        SIDEBAR_SEARCH_ID,
    };
    use zode_node_protocol::WorkspaceUri;

    use super::widget_command_for_snapshot;

    #[test]
    fn sidebar_search_trigger_opens_search_with_default_enter_target() {
        let mut state = demo_state();
        let snapshot = WorkspaceSnapshot::build(&state, 1_200.0, 900.0, Insets::ZERO);
        let rect = snapshot
            .node(SIDEBAR_SEARCH_ID)
            .expect("sidebar search trigger")
            .rect;
        let point = Point2D::new(
            rect.origin.x + rect.size.x / 2.0,
            rect.origin.y + rect.size.y / 2.0,
        );
        let hit = snapshot.hit_test(point).expect("sidebar search hit");

        assert_eq!(hit, SIDEBAR_SEARCH_ID);
        let command = widget_command_for_snapshot(&state, &snapshot, hit);
        assert_eq!(command, Some(AppCommand::ToggleGlobalSearch));
        assert_eq!(
            reduce_navigation_command(&mut state, command.expect("search command")),
            NavigationOutcome::Applied,
        );

        let opened = WorkspaceSnapshot::build(&state, 1_200.0, 900.0, Insets::ZERO);
        assert!(opened.node(GLOBAL_SEARCH_SURFACE_ID).is_some());
        assert!(opened.node(GLOBAL_SEARCH_INPUT_ID).is_some());
        let view = GlobalSearchViewState {
            open: state.global_search.open,
            query: state.global_search.query.clone(),
            active_index: state.global_search.active_index,
        };
        let search_layout =
            GlobalSearch::layout(opened.layout.viewport, &state, &view).expect("search layout");
        assert_eq!(
            GlobalSearch::command_for_active(&state, &view, &search_layout),
            Some(AppCommand::BeginTask {
                workspace_uri: None,
            }),
        );
    }

    #[test]
    fn visible_composer_footer_controls_dispatch_their_menu_commands() {
        let state = demo_state();
        let snapshot = WorkspaceSnapshot::build(&state, 1_200.0, 900.0, Insets::ZERO);
        for (id, menu) in [
            (COMPOSER_ADD_ID, ComposerFooterMenu::Add),
            (COMPOSER_PERMISSION_ID, ComposerFooterMenu::Permission),
            (COMPOSER_MODEL_ID, ComposerFooterMenu::Model),
        ] {
            let rect = snapshot.node(id).expect("footer trigger").rect;
            let point = Point2D::new(
                rect.origin.x + rect.size.x / 2.0,
                rect.origin.y + rect.size.y / 2.0,
            );
            let hit = snapshot.hit_test(point).expect("footer trigger hit");
            assert_eq!(hit, id);
            assert_eq!(
                widget_command_for_snapshot(&state, &snapshot, hit),
                Some(AppCommand::ToggleComposerFooterMenu(menu))
            );
        }
    }

    #[test]
    fn visible_composer_controls_open_their_menu_surfaces_end_to_end() {
        let workspace = WorkspaceUri::new("file:///tmp/zode-composer-menu").unwrap();
        let mut base = demo_state();
        base.projects = vec![ProjectState {
            workspace_uri: workspace.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 0,
        }];
        base.active_workspace = Some(workspace);
        base.current_session = None;
        base.composer.selected_branch = Some("main".into());

        for (trigger, surface) in [
            (COMPOSER_PROJECT_ID, PROJECT_PICKER_SURFACE_ID),
            (COMPOSER_LOCATION_ID, COMPOSER_CONTEXT_MENU_SURFACE_ID),
            (COMPOSER_BRANCH_ID, COMPOSER_CONTEXT_MENU_SURFACE_ID),
            (COMPOSER_ADD_ID, COMPOSER_FOOTER_MENU_SURFACE_ID),
            (COMPOSER_PERMISSION_ID, COMPOSER_FOOTER_MENU_SURFACE_ID),
            (COMPOSER_MODEL_ID, COMPOSER_FOOTER_MENU_SURFACE_ID),
        ] {
            let mut state = base.clone();
            open_surface_from_trigger(&mut state, trigger);
            let opened = WorkspaceSnapshot::build(&state, 1_200.0, 900.0, Insets::ZERO);
            assert!(opened.node(surface).is_some(), "surface for {trigger:?}");
        }

        assert_eq!(base.composer.context_menu, None);
        assert_eq!(base.composer.footer_menu, None);
        assert!(!base.project_picker.open);

        fn open_surface_from_trigger(state: &mut zode_app_model::ZodeAppState, trigger: WidgetId) {
            let snapshot = WorkspaceSnapshot::build(state, 1_200.0, 900.0, Insets::ZERO);
            let rect = snapshot.node(trigger).expect("composer trigger").rect;
            let point = Point2D::new(
                rect.origin.x + rect.size.x / 2.0,
                rect.origin.y + rect.size.y / 2.0,
            );
            let hit = snapshot.hit_test(point).expect("composer trigger hit");
            assert_eq!(hit, trigger);
            let command = widget_command_for_snapshot(state, &snapshot, hit)
                .expect("composer trigger command");
            assert_eq!(
                reduce_navigation_command(state, command),
                NavigationOutcome::Applied,
            );
        }
    }

    #[test]
    fn projectless_composer_project_chip_opens_picker_end_to_end() {
        let workspace = WorkspaceUri::new("file:///tmp/zode-project-choice").unwrap();
        let mut state = demo_state();
        state.projects = vec![ProjectState {
            workspace_uri: workspace.clone(),
            expanded: true,
            available: true,
            last_opened_ms: 0,
        }];
        state.active_workspace = None;
        state.current_session = None;

        let closed = WorkspaceSnapshot::build(&state, 1_200.0, 900.0, Insets::ZERO);
        assert!(closed.node(PROJECT_DETACH_ID).is_none());
        let trigger = closed
            .node(COMPOSER_PROJECT_ID)
            .expect("projectless select-project trigger");
        let point = Point2D::new(
            trigger.rect.origin.x + trigger.rect.size.x / 2.0,
            trigger.rect.origin.y + trigger.rect.size.y / 2.0,
        );
        let hit = closed.hit_test(point).expect("select-project trigger hit");
        assert_eq!(hit, COMPOSER_PROJECT_ID);
        let command = widget_command_for_snapshot(&state, &closed, hit)
            .expect("select-project trigger command");
        assert_eq!(command, AppCommand::ToggleComposerProjectPicker);
        assert_eq!(
            reduce_navigation_command(&mut state, command),
            NavigationOutcome::Applied,
        );

        let opened = WorkspaceSnapshot::build(&state, 1_200.0, 900.0, Insets::ZERO);
        assert!(opened.node(PROJECT_PICKER_SURFACE_ID).is_some());
        assert!(opened.node(PROJECT_PICKER_SEARCH_ID).is_some());
        assert!(opened.node(PROJECT_PICKER_NEW_ID).is_some());
        assert!(opened.node(PROJECT_PICKER_PROJECTLESS_ID).is_none());
        assert_eq!(opened.focused, Some(PROJECT_PICKER_SEARCH_ID));

        let project_id = ProjectPicker::project_widget_id(&workspace);
        let project = opened.node(project_id).expect("existing project choice");
        let project_point = Point2D::new(
            project.rect.origin.x + project.rect.size.x / 2.0,
            project.rect.origin.y + project.rect.size.y / 2.0,
        );
        let project_hit = opened.hit_test(project_point).expect("project choice hit");
        assert_eq!(project_hit, project_id);
        let choose = widget_command_for_snapshot(&state, &opened, project_hit)
            .expect("existing project choice command");
        assert_eq!(
            choose,
            AppCommand::BeginTask {
                workspace_uri: Some(workspace.clone()),
            }
        );
        assert_eq!(
            reduce_navigation_command(&mut state, choose),
            NavigationOutcome::Applied,
        );

        let selected = WorkspaceSnapshot::build(&state, 1_200.0, 900.0, Insets::ZERO);
        let chip = selected
            .node(COMPOSER_PROJECT_ID)
            .expect("selected project composer chip");
        assert_eq!(chip.value.as_deref(), Some("zode-project-choice"));
        assert!(selected.node(PROJECT_DETACH_ID).is_some());
        assert!(selected.node(PROJECT_PICKER_SURFACE_ID).is_none());
    }
}
