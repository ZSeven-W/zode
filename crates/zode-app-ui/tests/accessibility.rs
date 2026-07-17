use std::collections::BTreeSet;

use accesskit::{Action, NodeId, Role};
use jian_widgets::{Point2D, Rect};
use zode_app_ui::{
    accessibility_tree, FocusDirection, Insets, InteractionNode, RectExt, WidgetId,
    WorkspaceLayout, WorkspaceSnapshot,
};

const NAVIGATION_ID: WidgetId = WidgetId(10);
const COMPOSER_ID: WidgetId = WidgetId(20);
const SEND_ID: WidgetId = WidgetId(30);

fn interaction_fixture() -> WorkspaceSnapshot {
    let layout = WorkspaceLayout::compute(1221.0, 992.0, Insets::ZERO);
    let composer_rect = Rect::xywh(
        layout.composer.min_x() + 7.0,
        layout.composer.min_y() + 11.0,
        layout.composer.width() - 14.0,
        layout.composer.height() - 22.0,
    );
    WorkspaceSnapshot {
        layout,
        nodes: vec![
            InteractionNode {
                id: NAVIGATION_ID,
                rect: layout.sidebar,
                role: Role::Navigation,
                name: "项目".into(),
                value: None,
                actions: vec![Action::Focus],
                focus_order: Some(0),
            },
            InteractionNode {
                id: COMPOSER_ID,
                rect: composer_rect,
                role: Role::TextInput,
                name: "要求后续变更".into(),
                value: Some("draft".into()),
                actions: vec![Action::Focus, Action::SetValue],
                focus_order: Some(1),
            },
            InteractionNode {
                id: SEND_ID,
                rect: Rect::xywh(
                    layout.composer.max_x() - 40.0,
                    layout.composer.max_y() - 40.0,
                    32.0,
                    32.0,
                ),
                role: Role::Button,
                name: "发送".into(),
                value: None,
                actions: vec![Action::Click, Action::Focus],
                focus_order: Some(2),
            },
        ],
        focused: Some(COMPOSER_ID),
    }
}

#[test]
fn generated_widget_ids_are_stable_unique_and_cover_core_interactions() {
    let state = zode_app_model::demo_state();
    let first = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    let second = WorkspaceSnapshot::build(&state, 1221.0, 992.0, Insets::ZERO);
    let first_ids = first.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
    let second_ids = second.nodes.iter().map(|node| node.id).collect::<Vec<_>>();
    let unique = first_ids.iter().copied().collect::<BTreeSet<_>>();

    assert_eq!(unique.len(), first_ids.len(), "WidgetId values are unique");
    assert!(
        first_ids.iter().all(|id| id.0 != 0),
        "interactive WidgetId values never collide with the AccessKit root",
    );
    assert_eq!(first_ids, second_ids, "WidgetId values are stable");
    assert!(first_ids.len() >= 3, "core shell interactions are present");
}

#[test]
fn generated_nodes_carry_role_name_value_actions_and_layout_rect() {
    let snapshot =
        WorkspaceSnapshot::build(&zode_app_model::demo_state(), 1221.0, 992.0, Insets::ZERO);
    let composer = snapshot
        .nodes
        .iter()
        .find(|node| node.role == Role::TextInput)
        .expect("composer interaction node");

    assert_eq!(composer.name, "要求后续变更");
    assert_eq!(composer.value.as_deref(), Some(""));
    assert!(composer.actions.contains(&Action::Focus));
    assert!(composer.actions.contains(&Action::SetValue));
    assert_eq!(composer.rect, snapshot.layout.composer);
}

#[test]
fn hit_testing_reads_interaction_rects_from_the_snapshot() {
    let snapshot = interaction_fixture();
    let composer_rect = snapshot.node(COMPOSER_ID).unwrap().rect;
    let center = Point2D::new(
        composer_rect.min_x() + composer_rect.width() / 2.0,
        composer_rect.min_y() + composer_rect.height() / 2.0,
    );
    let layout_only_point = Point2D::new(
        snapshot.layout.composer.min_x() + 2.0,
        snapshot.layout.composer.min_y() + 2.0,
    );

    assert_eq!(snapshot.hit_test(layout_only_point), None);
    assert_eq!(snapshot.hit_test(center), Some(COMPOSER_ID));
}

#[test]
fn focus_order_and_tab_traversal_work_in_both_directions() {
    let snapshot = interaction_fixture();

    assert_eq!(
        snapshot.focusable_ids(),
        vec![NAVIGATION_ID, COMPOSER_ID, SEND_ID],
    );
    assert_eq!(
        snapshot.move_focus(Some(NAVIGATION_ID), FocusDirection::Forward),
        Some(COMPOSER_ID),
    );
    assert_eq!(
        snapshot.move_focus(Some(COMPOSER_ID), FocusDirection::Backward),
        Some(NAVIGATION_ID),
    );
    assert_eq!(
        snapshot.move_focus(Some(SEND_ID), FocusDirection::Forward),
        Some(NAVIGATION_ID),
    );
    assert_eq!(
        snapshot.move_focus(Some(NAVIGATION_ID), FocusDirection::Backward),
        Some(SEND_ID),
    );
}

#[test]
fn accesskit_tree_uses_physical_root_bounds() {
    let snapshot =
        WorkspaceSnapshot::build(&zode_app_model::demo_state(), 390.0, 844.0, Insets::ZERO);
    let update = accessibility_tree(&snapshot, 2.0);
    let (_, root) = update.nodes.first().expect("tree has a window root");
    let bounds = root.bounds().expect("root carries physical bounds");

    assert_eq!((bounds.x0, bounds.y0), (0.0, 0.0));
    assert_eq!((bounds.x1, bounds.y1), (780.0, 1688.0));
}

#[test]
fn accesskit_composer_node_preserves_id_physical_rect_and_semantics() {
    let snapshot = interaction_fixture();
    let update = accessibility_tree(&snapshot, 2.0);
    let (_, composer) = update
        .nodes
        .iter()
        .find(|(id, _)| *id == NodeId(COMPOSER_ID.0))
        .expect("composer maps WidgetId directly to AccessKit NodeId");
    let bounds = composer.bounds().expect("composer has physical bounds");

    assert_eq!(composer.role(), Role::TextInput);
    assert_eq!(composer.label(), Some("要求后续变更"));
    assert_eq!(composer.value(), Some("draft"));
    assert!(composer.supports_action(Action::Focus));
    assert!(composer.supports_action(Action::SetValue));
    let composer_rect = snapshot.node(COMPOSER_ID).unwrap().rect;
    assert_eq!(
        (bounds.x0, bounds.y0, bounds.x1, bounds.y1),
        (
            f64::from(composer_rect.min_x()) * 2.0,
            f64::from(composer_rect.min_y()) * 2.0,
            f64::from(composer_rect.max_x()) * 2.0,
            f64::from(composer_rect.max_y()) * 2.0,
        ),
    );
}
