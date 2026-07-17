use accesskit::{Node, NodeId, Role, Tree, TreeId, TreeUpdate};
use jian_widgets::Rect;

use super::WorkspaceSnapshot;
use crate::GLOBAL_SEARCH_SURFACE_ID;

pub fn accessibility_tree(snapshot: &WorkspaceSnapshot, physical_scale: f64) -> TreeUpdate {
    let scale = if physical_scale.is_finite() && physical_scale > 0.0 {
        physical_scale
    } else {
        1.0
    };
    let root_id = NodeId(0);
    let global_search_surface = snapshot
        .nodes
        .iter()
        .position(|node| node.id == GLOBAL_SEARCH_SURFACE_ID);
    let exposed_nodes = global_search_surface.map_or(snapshot.nodes.as_slice(), |surface_index| {
        &snapshot.nodes[surface_index..]
    });
    let child_ids = global_search_surface.map_or_else(
        || {
            exposed_nodes
                .iter()
                .map(|node| NodeId(node.id.0))
                .collect::<Vec<_>>()
        },
        |_| vec![NodeId(GLOBAL_SEARCH_SURFACE_ID.0)],
    );
    let mut root = Node::new(Role::Window);
    root.set_label("Zode");
    root.set_bounds(physical_rect(snapshot.layout.viewport, scale));
    root.set_children(child_ids);

    let mut nodes = Vec::with_capacity(exposed_nodes.len() + 1);
    nodes.push((root_id, root));
    for (index, source) in exposed_nodes.iter().enumerate() {
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
        if source.disabled {
            target.set_disabled();
        }
        if source.id == GLOBAL_SEARCH_SURFACE_ID {
            target.set_modal();
            target.set_children(
                exposed_nodes[index + 1..]
                    .iter()
                    .map(|child| NodeId(child.id.0))
                    .collect::<Vec<_>>(),
            );
        }
        nodes.push((NodeId(source.id.0), target));
    }

    let focus = snapshot
        .focused
        .filter(|focused| exposed_nodes.iter().any(|node| node.id == *focused))
        .map_or(root_id, |focused| NodeId(focused.0));
    TreeUpdate {
        nodes,
        tree: Some(Tree::new(root_id)),
        tree_id: TreeId::ROOT,
        focus,
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
