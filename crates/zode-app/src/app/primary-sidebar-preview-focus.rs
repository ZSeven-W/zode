use zode_app_ui::{ProjectSidebar, WidgetId};

use super::{sidebar_menu::sidebar_menu_close_command, DesktopApp};

impl DesktopApp {
    /// Returns whether a sidebar menu is actually visible to keyboard users.
    ///
    /// Menu state may briefly outlive a closing transient preview. That stale
    /// state must not trap Tab, arrows, Escape, or activation keys after the
    /// surface is gone.
    pub(super) fn primary_sidebar_menu_accepts_keyboard(&self) -> bool {
        self.app_state.shell.sidebar_open || self.primary_sidebar_preview_is_visible()
    }

    pub(super) fn primary_sidebar_menu_focused_widget(&self) -> Option<WidgetId> {
        if self.app_state.shell.sidebar_open {
            self.focused_widget
        } else {
            self.primary_sidebar_preview_focus
                .filter(|id| self.primary_sidebar_menu_focusable_ids().contains(id))
        }
    }

    pub(super) fn set_primary_sidebar_menu_focus(&mut self, focused: Option<WidgetId>) {
        if self.app_state.shell.sidebar_open {
            self.set_focused_widget(focused);
            return;
        }

        let focused = focused.filter(|id| self.primary_sidebar_menu_focusable_ids().contains(id));
        if self.primary_sidebar_preview_focus == focused {
            return;
        }
        self.primary_sidebar_preview_focus = focused;
        self.ime.invalidate_native();
        self.request_redraw();
    }

    /// Focus used only while painting the transient sidebar. The immutable
    /// workspace snapshot intentionally does not contain overlay menu nodes.
    pub(super) fn primary_sidebar_preview_paint_focus(&self) -> Option<WidgetId> {
        self.primary_sidebar_preview_focus
            .filter(|id| self.primary_sidebar_menu_focusable_ids().contains(id))
            .or(self.focused_widget)
    }

    /// Clears host-only focus and closes any menu that belonged to a collapsed
    /// sidebar. Call this after a forced dismissal or once the closing
    /// animation reaches zero.
    pub(super) fn clear_primary_sidebar_preview_interaction(&mut self) -> bool {
        let focus_changed = self.primary_sidebar_preview_focus.take().is_some();
        let close = (!self.app_state.shell.sidebar_open)
            .then(|| sidebar_menu_close_command(&self.app_state))
            .flatten();
        if let Some(command) = close {
            self.enqueue_command(command);
            return true;
        }
        if focus_changed {
            self.request_redraw();
        }
        focus_changed
    }

    fn primary_sidebar_menu_focusable_ids(&self) -> Vec<WidgetId> {
        ProjectSidebar::menu_layout(self.active_primary_sidebar_rect(), &self.app_state)
            .map(|menu| {
                menu.items
                    .into_iter()
                    .filter(|item| item.enabled)
                    .map(|item| item.id)
                    .collect()
            })
            .unwrap_or_default()
    }
}
