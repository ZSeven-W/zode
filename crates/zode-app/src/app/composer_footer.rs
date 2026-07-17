use zode_app_model::{AppCommand, ComposerFooterMenu, ZodeAppState};
use zode_app_ui::{
    ComposerFooterMenuWidget, Key, KeyEvent, Modifiers, WidgetId, WorkspaceSnapshot,
    COMPOSER_ADD_ID, COMPOSER_FOOTER_MENU_SURFACE_ID, COMPOSER_MODEL_ID, COMPOSER_PERMISSION_ID,
};

use super::DesktopApp;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FooterPointerOutcome {
    Ignored,
    Actionable,
    Captured,
    Close,
}

fn pointer_outcome(
    state: &ZodeAppState,
    snapshot: &WorkspaceSnapshot,
    point: jian_widgets::Point2D,
) -> FooterPointerOutcome {
    if state.composer.footer_menu.is_none() {
        return FooterPointerOutcome::Ignored;
    }
    let hit = snapshot.hit_test(point);
    let Some(surface) = snapshot
        .node(COMPOSER_FOOTER_MENU_SURFACE_ID)
        .map(|node| node.rect)
    else {
        return FooterPointerOutcome::Close;
    };
    if surface.contains(point) {
        return if hit
            .is_some_and(|id| ComposerFooterMenuWidget::command_for_widget(state, id).is_some())
        {
            FooterPointerOutcome::Actionable
        } else {
            FooterPointerOutcome::Captured
        };
    }
    if hit.is_some_and(is_trigger) {
        FooterPointerOutcome::Actionable
    } else {
        FooterPointerOutcome::Close
    }
}

fn is_trigger(id: WidgetId) -> bool {
    matches!(
        id,
        COMPOSER_ADD_ID | COMPOSER_PERMISSION_ID | COMPOSER_MODEL_ID
    )
}

impl DesktopApp {
    pub(super) fn handle_composer_footer_pointer(&mut self, point: jian_widgets::Point2D) -> bool {
        match pointer_outcome(&self.app_state, &self.frame_snapshot, point) {
            FooterPointerOutcome::Ignored | FooterPointerOutcome::Actionable => false,
            FooterPointerOutcome::Captured => true,
            FooterPointerOutcome::Close => {
                self.enqueue_command(AppCommand::CloseComposerFooterMenu);
                true
            }
        }
    }

    pub(super) fn composer_footer_allows_accessibility_action(&self, id: WidgetId) -> bool {
        if self.app_state.composer.footer_menu.is_none() {
            return true;
        }
        self.frame_snapshot.node(id).is_some_and(|node| {
            !node.disabled
                && !node.actions.is_empty()
                && (is_trigger(id)
                    || ComposerFooterMenuWidget::command_for_widget(&self.app_state, id).is_some())
        })
    }

    pub(super) fn handle_composer_footer_key(&mut self, event: &KeyEvent) -> bool {
        if self.app_state.composer.footer_menu.is_none() || !event.pressed {
            return false;
        }
        if event.key == Key::Escape {
            self.enqueue_command(AppCommand::CloseComposerFooterMenu);
            return true;
        }
        if event.key == Key::Tab || matches!(event.key, Key::ArrowUp | Key::ArrowDown) {
            let backwards = event.modifiers.contains(Modifiers::SHIFT) || event.key == Key::ArrowUp;
            self.cycle_composer_footer_focus(backwards);
            return true;
        }
        false
    }

    pub(super) fn sync_composer_footer_after_navigation(
        &self,
        command: &AppCommand,
        previous: Option<ComposerFooterMenu>,
    ) -> Option<WidgetId> {
        let trigger = |menu| match menu {
            ComposerFooterMenu::Add => COMPOSER_ADD_ID,
            ComposerFooterMenu::Permission => COMPOSER_PERMISSION_ID,
            ComposerFooterMenu::Model
            | ComposerFooterMenu::ModelModels
            | ComposerFooterMenu::ModelEffort
            | ComposerFooterMenu::ModelSpeed => COMPOSER_MODEL_ID,
        };
        match command {
            AppCommand::ToggleComposerFooterMenu(menu) => Some(trigger(*menu)),
            AppCommand::CloseComposerFooterMenu => previous.map(trigger),
            AppCommand::SetModel(_)
            | AppCommand::SetEffort(_)
            | AppCommand::SetSandbox { .. }
            | AppCommand::SetPermissionPreset { .. }
            | AppCommand::ResetComposerRuntime => previous.map(trigger),
            _ => None,
        }
    }

    fn footer_focus_ids(&self) -> Vec<WidgetId> {
        let Some(surface) = self
            .frame_snapshot
            .node(COMPOSER_FOOTER_MENU_SURFACE_ID)
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

    fn cycle_composer_footer_focus(&mut self, backwards: bool) {
        let ids = self.footer_focus_ids();
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
