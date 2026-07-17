use accesskit::{Action, Node, NodeId, Role, Tree, TreeId, TreeUpdate};
use jian_widgets::{Point2D, Rect};
use zode_app_model::ZodeAppState;

use crate::{Insets, WorkspaceLayout};

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
    pub fn build(_state: &ZodeAppState, width: f32, height: f32, insets: Insets) -> Self {
        Self {
            layout: WorkspaceLayout::compute(width, height, insets),
            nodes: Vec::new(),
            focused: None,
        }
    }

    pub fn node(&self, id: WidgetId) -> Option<&InteractionNode> {
        self.nodes.iter().find(|node| node.id == id)
    }

    pub fn hit_test(&self, point: Point2D) -> Option<WidgetId> {
        let _ = point;
        None
    }

    pub fn focusable_ids(&self) -> Vec<WidgetId> {
        Vec::new()
    }

    pub fn move_focus(
        &self,
        _current: Option<WidgetId>,
        _direction: FocusDirection,
    ) -> Option<WidgetId> {
        None
    }
}

pub fn accessibility_tree(snapshot: &WorkspaceSnapshot, _physical_scale: f64) -> TreeUpdate {
    let root_id = NodeId(0);
    let mut root = Node::new(Role::Window);
    root.set_label("Zode");
    let viewport = snapshot.layout.viewport;
    root.set_bounds(accesskit::Rect {
        x0: viewport.origin.x as f64,
        y0: viewport.origin.y as f64,
        x1: (viewport.origin.x + viewport.size.x) as f64,
        y1: (viewport.origin.y + viewport.size.y) as f64,
    });
    TreeUpdate {
        nodes: vec![(root_id, root)],
        tree: Some(Tree::new(root_id)),
        tree_id: TreeId::ROOT,
        focus: root_id,
    }
}
