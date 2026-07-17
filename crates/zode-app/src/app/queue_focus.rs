use zode_app_model::{AppCommand, QueuedMessageId};
use zode_app_ui::{Composer, WidgetId};
use zode_node_protocol::SessionLocator;

use super::DesktopApp;

impl DesktopApp {
    pub(super) fn focus_open_queue_menu(&mut self, message_id: QueuedMessageId) {
        let target = Composer::queue_layout(self.frame_snapshot.layout.composer, &self.app_state)
            .and_then(|layout| layout.menu)
            .filter(|menu| menu.message_id == message_id)
            .map(|menu| menu.edit_id);
        if let Some(target) = target {
            self.set_focused_widget(Some(target));
        }
    }

    pub(super) fn close_queue_menu_and_restore_focus(
        &mut self,
        session: SessionLocator,
        message_id: QueuedMessageId,
    ) {
        let trigger = self.queue_more_widget(message_id);
        self.enqueue_command(AppCommand::ToggleQueuedMessageMenu {
            session,
            id: message_id,
        });
        if let Some(trigger) = trigger {
            self.set_focused_widget(Some(trigger));
        }
    }

    pub(super) fn trap_queue_menu_tab(&mut self, backwards: bool) -> bool {
        let menu = Composer::queue_layout(self.frame_snapshot.layout.composer, &self.app_state)
            .and_then(|layout| layout.menu);
        let Some(menu) = menu else {
            return false;
        };
        let order = [menu.edit_id, menu.close_id];
        let target = next_menu_focus(order, self.focused_widget, backwards);
        self.set_focused_widget(Some(target));
        true
    }

    fn queue_more_widget(&self, message_id: QueuedMessageId) -> Option<WidgetId> {
        Composer::queue_layout(self.frame_snapshot.layout.composer, &self.app_state)
            .and_then(|layout| {
                layout
                    .rows
                    .into_iter()
                    .find(|row| row.message_id == message_id)
            })
            .map(|row| row.more_id)
    }
}

fn next_menu_focus(order: [WidgetId; 2], current: Option<WidgetId>, backwards: bool) -> WidgetId {
    let current = current.and_then(|focused| order.iter().position(|id| *id == focused));
    let index = match (backwards, current) {
        (false, Some(index)) => (index + 1) % order.len(),
        (true, Some(0) | None) => order.len() - 1,
        (true, Some(index)) => index - 1,
        (false, None) => 0,
    };
    order[index]
}

#[cfg(test)]
mod tests {
    use zode_app_ui::WidgetId;

    use super::next_menu_focus;

    #[test]
    fn queue_menu_tab_focus_wraps_inside_the_two_menu_items() {
        let edit = WidgetId(1);
        let close = WidgetId(2);
        let order = [edit, close];

        assert_eq!(next_menu_focus(order, None, false), edit);
        assert_eq!(next_menu_focus(order, Some(edit), false), close);
        assert_eq!(next_menu_focus(order, Some(close), false), edit);
        assert_eq!(next_menu_focus(order, Some(edit), true), close);
        assert_eq!(next_menu_focus(order, Some(close), true), edit);
    }
}
