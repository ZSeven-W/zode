use std::hash::{Hash, Hasher};

use accesskit::{Action, Node, NodeId, Role, Toggled, Tree, TreeId, TreeUpdate};
use jian_core::CursorHint;
use jian_widgets::{Point2D, Rect};
use zode_app_model::{ShellPage, TranscriptItem, ZodeAppState};

use crate::{
    ApprovalCard, Insets, ProjectSidebar, RectExt, SettingsPanel, SidebarRowTarget,
    ThreadTranscript, ToolCard, WorkspaceLayout,
};

pub const SIDEBAR_ID: WidgetId = WidgetId(1);
pub const NEW_SESSION_ID: WidgetId = WidgetId(2);
pub const WORKFLOWS_NAV_ID: WidgetId = WidgetId(3);
pub const PLUGINS_NAV_ID: WidgetId = WidgetId(4);
pub const OPENPENCIL_NAV_ID: WidgetId = WidgetId(5);
pub const BROWSER_NAV_ID: WidgetId = WidgetId(6);
pub const SETTINGS_NAV_ID: WidgetId = WidgetId(7);
pub const SETTINGS_ROOT_ID: WidgetId = WidgetId(8);
pub const COMPOSER_ID: WidgetId = WidgetId(20);
pub const SEND_ID: WidgetId = WidgetId(21);
pub const TERMINAL_ID: WidgetId = WidgetId(30);
pub const THEME_SYSTEM_ID: WidgetId = WidgetId(40);
pub const THEME_LIGHT_ID: WidgetId = WidgetId(41);
pub const THEME_DARK_ID: WidgetId = WidgetId(42);
pub const REDUCED_MOTION_ID: WidgetId = WidgetId(43);
pub const HIGH_CONTRAST_ID: WidgetId = WidgetId(44);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WidgetId(pub u64);

#[derive(Debug, Clone, PartialEq)]
pub struct InteractionNode {
    pub id: WidgetId,
    pub rect: Rect,
    pub role: Role,
    pub name: String,
    pub value: Option<String>,
    pub actions: Vec<Action>,
    pub focus_order: Option<u32>,
    pub cursor: CursorHint,
    pub toggled: Option<Toggled>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusDirection {
    Forward,
    Backward,
}

/// One immutable interaction snapshot consumed by paint, hit testing and a11y.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkspaceSnapshot {
    pub layout: WorkspaceLayout,
    pub nodes: Vec<InteractionNode>,
    pub focused: Option<WidgetId>,
}

impl WorkspaceSnapshot {
    pub fn build(state: &ZodeAppState, width: f32, height: f32, insets: Insets) -> Self {
        let layout = WorkspaceLayout::compute(width, height, insets);
        let mut nodes = Vec::new();
        let mut focus_order = 0;

        if layout.sidebar.size.x > 0.0 && state.shell.page != ShellPage::Settings {
            nodes.push(node(
                SIDEBAR_ID,
                layout.sidebar,
                Role::Navigation,
                "项目",
                None,
                Vec::new(),
                None,
                CursorHint::Default,
            ));
            for (row, (id, enabled)) in ProjectSidebar::navigation_row_layout(layout.sidebar)
                .into_iter()
                .zip([
                    (NEW_SESSION_ID, true),
                    (WORKFLOWS_NAV_ID, false),
                    (PLUGINS_NAV_ID, false),
                    (OPENPENCIL_NAV_ID, false),
                    (BROWSER_NAV_ID, false),
                    (SETTINGS_NAV_ID, true),
                ])
            {
                nodes.push(node(
                    id,
                    row.rect,
                    Role::Button,
                    row.item.label,
                    None,
                    if enabled {
                        vec![Action::Click, Action::Focus]
                    } else {
                        Vec::new()
                    },
                    if enabled {
                        next_order(&mut focus_order)
                    } else {
                        None
                    },
                    if enabled {
                        CursorHint::Pointer
                    } else {
                        CursorHint::NotAllowed
                    },
                ));
            }
            for row in ProjectSidebar::dynamic_row_layout(layout.sidebar, state) {
                let name = match &row.target {
                    SidebarRowTarget::Project(_) => format!("项目 {}", row.label),
                    SidebarRowTarget::Session(_) => row.label.clone(),
                };
                nodes.push(node(
                    row.id,
                    row.rect,
                    Role::Button,
                    &name,
                    None,
                    if row.actionable {
                        vec![Action::Click, Action::Focus]
                    } else {
                        Vec::new()
                    },
                    if row.actionable {
                        next_order(&mut focus_order)
                    } else {
                        None
                    },
                    if row.actionable {
                        CursorHint::Pointer
                    } else {
                        CursorHint::Default
                    },
                ));
            }
        }

        let focused = match state.shell.page {
            ShellPage::Settings => {
                append_settings_nodes(&mut nodes, &layout, &mut focus_order, state);
                nodes
                    .iter()
                    .find(|node| node.focus_order.is_some())
                    .map(|node| node.id)
            }
            ShellPage::Terminal => {
                let rect = Rect::xywh(
                    layout.transcript.origin.x,
                    layout.transcript.origin.y,
                    layout.transcript.size.x,
                    layout.composer.max_y() - layout.transcript.origin.y,
                );
                nodes.push(node(
                    TERMINAL_ID,
                    rect,
                    Role::TextInput,
                    "终端",
                    None,
                    vec![Action::Focus],
                    next_order(&mut focus_order),
                    CursorHint::Text,
                ));
                Some(TERMINAL_ID)
            }
            _ => {
                append_transcript_nodes(&mut nodes, &layout, &mut focus_order, state);
                nodes.push(node(
                    COMPOSER_ID,
                    layout.composer,
                    Role::TextInput,
                    "要求后续变更",
                    Some(state.composer.draft.clone()),
                    vec![Action::Focus, Action::SetValue],
                    next_order(&mut focus_order),
                    CursorHint::Text,
                ));
                nodes.push(node(
                    SEND_ID,
                    Rect::xywh(
                        layout.composer.max_x() - 42.0,
                        layout.composer.max_y() - 38.0,
                        28.0,
                        28.0,
                    ),
                    Role::Button,
                    "发送",
                    None,
                    vec![Action::Click, Action::Focus],
                    next_order(&mut focus_order),
                    CursorHint::Pointer,
                ));
                Some(COMPOSER_ID)
            }
        };

        Self {
            layout,
            nodes,
            focused,
        }
    }

    pub fn node(&self, id: WidgetId) -> Option<&InteractionNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn hit_test(&self, point: Point2D) -> Option<WidgetId> {
        self.nodes
            .iter()
            .rev()
            .find(|node| !node.actions.is_empty() && node.rect.contains(point))
            .map(|node| node.id)
    }

    pub fn focusable_ids(&self) -> Vec<WidgetId> {
        let mut ordered = self
            .nodes
            .iter()
            .filter_map(|node| node.focus_order.map(|order| (order, node.id)))
            .collect::<Vec<_>>();
        ordered.sort_by_key(|(order, _)| *order);
        ordered.into_iter().map(|(_, id)| id).collect()
    }

    pub fn move_focus(
        &self,
        current: Option<WidgetId>,
        direction: FocusDirection,
    ) -> Option<WidgetId> {
        let order = self.focusable_ids();
        if order.is_empty() {
            return None;
        }
        let current_index =
            current.and_then(|id| order.iter().position(|candidate| *candidate == id));
        let index = match (direction, current_index) {
            (FocusDirection::Forward, Some(index)) => (index + 1) % order.len(),
            (FocusDirection::Backward, Some(0)) => order.len() - 1,
            (FocusDirection::Backward, Some(index)) => index - 1,
            (FocusDirection::Forward, None) => 0,
            (FocusDirection::Backward, None) => order.len() - 1,
        };
        Some(order[index])
    }
}

/// Stable FNV-1a IDs occupy a namespace byte plus a deterministic 56-bit
/// payload. This never depends on `RandomState` or process entropy.
pub(crate) fn stable_widget_id<T: Hash + ?Sized>(namespace: u8, key: &T) -> WidgetId {
    let mut hasher = StableFnvHasher::new();
    namespace.hash(&mut hasher);
    key.hash(&mut hasher);
    let payload = hasher.finish() & 0x00ff_ffff_ffff_ffff;
    WidgetId((u64::from(namespace) << 56) | payload)
}

struct StableFnvHasher(u64);

impl StableFnvHasher {
    const fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Hasher for StableFnvHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

pub fn accessibility_tree(snapshot: &WorkspaceSnapshot, physical_scale: f64) -> TreeUpdate {
    let scale = if physical_scale.is_finite() && physical_scale > 0.0 {
        physical_scale
    } else {
        1.0
    };
    let root_id = NodeId(0);
    let child_ids = snapshot
        .nodes
        .iter()
        .map(|node| NodeId(node.id.0))
        .collect::<Vec<_>>();
    let mut root = Node::new(Role::Window);
    root.set_label("Zode");
    root.set_bounds(physical_rect(snapshot.layout.viewport, scale));
    root.set_children(child_ids);

    let mut nodes = Vec::with_capacity(snapshot.nodes.len() + 1);
    nodes.push((root_id, root));
    for source in &snapshot.nodes {
        let mut target = Node::new(source.role);
        target.set_label(source.name.clone());
        if let Some(value) = source.value.as_ref() {
            target.set_value(value.clone());
        }
        target.set_bounds(physical_rect(source.rect, scale));
        for action in source.actions.iter().copied() {
            target.add_action(action);
        }
        if let Some(toggled) = source.toggled {
            target.set_toggled(toggled);
        }
        nodes.push((NodeId(source.id.0), target));
    }

    let focus = snapshot
        .focused
        .filter(|focused| snapshot.node(*focused).is_some())
        .map_or(root_id, |focused| NodeId(focused.0));
    TreeUpdate {
        nodes,
        tree: Some(Tree::new(root_id)),
        tree_id: TreeId::ROOT,
        focus,
    }
}

#[allow(clippy::too_many_arguments)]
fn node(
    id: WidgetId,
    rect: Rect,
    role: Role,
    name: &str,
    value: Option<String>,
    actions: Vec<Action>,
    focus_order: Option<u32>,
    cursor: CursorHint,
) -> InteractionNode {
    InteractionNode {
        id,
        rect,
        role,
        name: name.into(),
        value,
        actions,
        focus_order,
        cursor,
        toggled: None,
    }
}

fn next_order(order: &mut u32) -> Option<u32> {
    let current = *order;
    *order = (*order).saturating_add(1);
    Some(current)
}

fn append_settings_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    nodes.push(node(
        SETTINGS_ROOT_ID,
        layout.transcript,
        Role::ScrollView,
        "设置内容",
        None,
        vec![Action::ScrollUp, Action::ScrollDown],
        None,
        CursorHint::Default,
    ));
    for control_layout in SettingsPanel::appearance_control_layout(layout.transcript, state) {
        let role = if matches!(
            control_layout.id,
            THEME_SYSTEM_ID | THEME_LIGHT_ID | THEME_DARK_ID
        ) {
            Role::RadioButton
        } else {
            Role::Switch
        };
        let mut control = node(
            control_layout.id,
            control_layout.visible_rect,
            role,
            &control_layout.control.label,
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        );
        control.toggled = Some(if control_layout.control.selected {
            Toggled::True
        } else {
            Toggled::False
        });
        nodes.push(control);
    }

    let Some(workspace_uri) = SettingsPanel::active_workspace_uri(state) else {
        return;
    };
    for row in SettingsPanel::permission_row_layout(layout.transcript, state, workspace_uri) {
        nodes.push(node(
            row.id,
            row.visible_rect,
            Role::Button,
            &format!("撤销 {} 权限", row.tool),
            None,
            vec![Action::Click, Action::Focus],
            next_order(focus_order),
            CursorHint::Pointer,
        ));
    }
}

fn append_transcript_nodes(
    nodes: &mut Vec<InteractionNode>,
    layout: &WorkspaceLayout,
    focus_order: &mut u32,
    state: &ZodeAppState,
) {
    let Some(session) = state.current_session.as_ref() else {
        return;
    };
    let Some(transcript) = state.transcripts.get(session) else {
        return;
    };
    for item_layout in ThreadTranscript::visible_item_layout_with_tools(
        layout.transcript,
        transcript,
        &state.tool_expanded,
    ) {
        let item = &transcript.items[item_layout.index];
        match item {
            TranscriptItem::UserText(text) => nodes.push(node(
                ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                item_layout.visible_rect,
                Role::Paragraph,
                &format!("你：{text}"),
                None,
                Vec::new(),
                None,
                CursorHint::Default,
            )),
            TranscriptItem::AssistantText(text) => nodes.push(node(
                ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                item_layout.visible_rect,
                Role::Paragraph,
                text,
                None,
                Vec::new(),
                None,
                CursorHint::Default,
            )),
            TranscriptItem::Thinking(text) => nodes.push(node(
                ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                item_layout.visible_rect,
                Role::Status,
                &format!("思考：{text}"),
                None,
                Vec::new(),
                None,
                CursorHint::Default,
            )),
            TranscriptItem::Tool(tool) => {
                let expanded = state
                    .tool_expanded
                    .get(&tool.id)
                    .copied()
                    .unwrap_or_else(|| ToolCard::default_expanded(tool));
                let mut control = node(
                    ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                    item_layout.visible_rect,
                    Role::Button,
                    &format!("{}：{}", tool.name, tool.summary),
                    None,
                    vec![Action::Click, Action::Focus],
                    next_order(focus_order),
                    CursorHint::Pointer,
                );
                control.toggled = Some(Toggled::from(expanded));
                nodes.push(control);
            }
            TranscriptItem::Approval { id, tool } => {
                nodes.push(node(
                    ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                    item_layout.visible_rect,
                    Role::Group,
                    &format!("需要批准：{tool}"),
                    None,
                    Vec::new(),
                    None,
                    CursorHint::Default,
                ));
                for button in ApprovalCard::button_layout(item_layout.rect) {
                    let Some(visible_button) =
                        ThreadTranscript::clip_to_viewport(button.rect, layout.transcript)
                    else {
                        continue;
                    };
                    nodes.push(node(
                        ThreadTranscript::approval_widget_id(session, id, button.action),
                        visible_button,
                        Role::Button,
                        button.label,
                        None,
                        vec![Action::Click, Action::Focus],
                        next_order(focus_order),
                        CursorHint::Pointer,
                    ));
                }
            }
            TranscriptItem::Status { message, .. } => nodes.push(node(
                ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                item_layout.visible_rect,
                Role::Status,
                message,
                None,
                Vec::new(),
                None,
                CursorHint::Default,
            )),
            TranscriptItem::Error { message, .. } => nodes.push(node(
                ThreadTranscript::semantic_widget_id(session, item_layout.index, item),
                item_layout.visible_rect,
                Role::Alert,
                message,
                None,
                Vec::new(),
                None,
                CursorHint::Default,
            )),
        }
    }
}

fn physical_rect(rect: Rect, scale: f64) -> accesskit::Rect {
    accesskit::Rect {
        x0: f64::from(rect.origin.x) * scale,
        y0: f64::from(rect.origin.y) * scale,
        x1: f64::from(rect.origin.x + rect.size.x) * scale,
        y1: f64::from(rect.origin.y + rect.size.y) * scale,
    }
}
