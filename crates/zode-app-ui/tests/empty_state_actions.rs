use accesskit::{Action, Role};
use jian_widgets::Rect;
use zode_app_model::{demo_state, ShellRoute};
use zode_app_ui::{EmptyState, Insets, RectExt, WorkspaceSnapshot, EMPTY_SUGGESTION_IDS};

#[test]
fn suggestion_cards_are_focusable_actions_with_stable_prompts() {
    let mut state = demo_state();
    state.current_session = None;
    state.presentation.route = ShellRoute::Conversation;

    let snapshot = WorkspaceSnapshot::build(&state, 1800.0, 1080.0, Insets::ZERO);
    let expected = [
        "探索并理解代码",
        "构建新功能、应用或工具",
        "审查代码并提出修改建议",
        "修复问题和失败",
    ];
    for (id, prompt) in EMPTY_SUGGESTION_IDS.into_iter().zip(expected) {
        let node = snapshot.node(id).expect("suggestion has an a11y node");
        assert_eq!(node.role, Role::Button);
        assert_eq!(node.name, prompt);
        assert_eq!(node.actions, vec![Action::Click, Action::Focus]);
        assert_eq!(EmptyState::suggestion_prompt(id), Some(prompt));
    }
}

#[test]
fn compact_suggestion_layout_keeps_four_non_overlapping_hit_targets() {
    let layouts = EmptyState::suggestion_layouts(Rect::xywh(200.0, 80.0, 520.0, 420.0));
    assert!(layouts
        .iter()
        .all(|layout| layout.rect.width() > 0.0 && layout.rect.height() > 0.0));
    for (index, layout) in layouts.iter().enumerate() {
        for other in layouts.iter().skip(index + 1) {
            let separated = layout.rect.max_x() <= other.rect.min_x()
                || other.rect.max_x() <= layout.rect.min_x()
                || layout.rect.max_y() <= other.rect.min_y()
                || other.rect.max_y() <= layout.rect.min_y();
            assert!(separated);
        }
    }
}

#[test]
fn narrow_suggestion_grid_keeps_the_reference_card_size_cap() {
    let wide = EmptyState::suggestion_layouts(Rect::xywh(0.0, 0.0, 736.0, 868.0));
    let narrow = EmptyState::suggestion_layouts(Rect::xywh(0.0, 0.0, 520.0, 868.0));
    let short = EmptyState::suggestion_layouts(Rect::xywh(0.0, 0.0, 736.0, 520.0));
    let reference_width = wide[0].rect.width();
    let reference_height = wide[0].rect.height();

    assert!(reference_width <= 170.6);
    assert!(reference_height <= 106.1);
    assert!(wide
        .iter()
        .all(|layout| (layout.rect.height() - reference_height).abs() <= 0.1));
    assert!(wide
        .iter()
        .all(|layout| layout.rect.min_y() == wide[0].rect.min_y()));
    assert!(
        narrow
            .iter()
            .all(|layout| (layout.rect.width() - reference_width).abs() <= 0.1),
        "narrow widths {:?} must preserve the {reference_width}px reference-card cap",
        narrow.map(|layout| layout.rect.width()),
    );
    assert!(
        narrow
            .iter()
            .all(|layout| (layout.rect.height() - reference_height).abs() <= 0.1),
        "narrow heights {:?} must preserve the {reference_height}px wide-card cap",
        narrow.map(|layout| layout.rect.height()),
    );
    assert_eq!(narrow[0].rect.min_y(), narrow[1].rect.min_y());
    assert_eq!(narrow[2].rect.min_y(), narrow[3].rect.min_y());
    assert!(narrow[2].rect.min_y() > narrow[0].rect.max_y());
    assert!(short.iter().all(|layout| {
        (layout.rect.width() - reference_width).abs() <= 0.1
            && (layout.rect.height() - reference_height).abs() <= 0.1
    }));
    assert_eq!(short[0].rect.min_y(), short[1].rect.min_y());
    assert_eq!(short[2].rect.min_y(), short[3].rect.min_y());
}

#[test]
fn disabled_task_suggestions_leave_no_hidden_actions() {
    let mut state = demo_state();
    state.current_session = None;
    state.presentation.route = ShellRoute::Conversation;
    state.ui_preferences.task_suggestions = false;

    let snapshot = WorkspaceSnapshot::build(&state, 1800.0, 1080.0, Insets::ZERO);
    assert!(EMPTY_SUGGESTION_IDS
        .into_iter()
        .all(|id| snapshot.node(id).is_none()));
}

#[test]
fn unknown_widget_is_not_a_suggestion() {
    assert_eq!(
        EmptyState::suggestion_prompt(zode_app_ui::WidgetId(999_999)),
        None
    );
}
