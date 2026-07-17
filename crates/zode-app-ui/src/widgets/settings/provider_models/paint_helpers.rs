use std::borrow::Cow;

use jian_widgets::{Painter, Rect};
use zode_app_model::{ProviderKindChoice, ProviderModelsStatus};

use crate::{Button, ButtonVariant, Input, SemanticIcon, ZodeTheme};

/// Radius this settings page's fields/buttons were already painted at
/// before the shared `components::{Button, Input}` migration - kept as an
/// explicit constant so adopting the shared components doesn't silently
/// resize these corners to the components' `sm` default.
const FIELD_RADIUS: f32 = 8.0;

/// Maximum number of dots painted for a pending (unsaved) secret, so a very
/// long pasted key does not blow out the field width.
const MAX_PENDING_SECRET_DOTS: usize = 12;

/// Chooses the display string for a secret (API key) field.
///
/// Dots proportional to `pending_len` (capped) take priority; they give live
/// feedback while typing/deleting. Falling back to the fixed placeholder dots
/// only when nothing has been typed but a credential is already on file, and
/// to the empty-state hint otherwise.
pub(super) fn secret_display(pending_len: usize, credential_configured: bool) -> Cow<'static, str> {
    if pending_len > 0 {
        Cow::Owned("•".repeat(pending_len.min(MAX_PENDING_SECRET_DOTS)))
    } else if credential_configured {
        Cow::Borrowed("••••••••")
    } else {
        Cow::Borrowed("输入密钥")
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn paint_input(
    painter: &mut dyn Painter,
    rect: Rect,
    value: &str,
    secret: bool,
    focused: bool,
    credential_configured: bool,
    pending_secret_len: usize,
    theme: &ZodeTheme,
) {
    let shown: Cow<str> = if secret {
        secret_display(pending_secret_len, credential_configured)
    } else if value.is_empty() {
        Cow::Borrowed("未设置")
    } else {
        Cow::Borrowed(value)
    };
    let muted = if secret {
        pending_secret_len == 0 && !credential_configured
    } else {
        value.is_empty()
    };
    // `components::Input` is the same focus-ring convention this field used
    // to hand-roll (see the settings search box in navigation.rs, and the
    // archived-tasks / global search / integrations search inputs, which
    // all render this way too).
    Input::paint(
        painter,
        rect,
        FIELD_RADIUS,
        shown.as_ref(),
        muted,
        focused,
        &theme.tokens,
    );
}

pub(super) fn paint_button(
    painter: &mut dyn Painter,
    rect: Rect,
    label: &str,
    icon: SemanticIcon,
    destructive: bool,
    disabled: bool,
    theme: &ZodeTheme,
) {
    // `Destructive` keeps this row's neutral secondary chrome and only
    // tints the icon/label red, matching this button's original look.
    let variant = if destructive {
        ButtonVariant::Destructive
    } else {
        ButtonVariant::Secondary
    };
    Button::paint(
        painter,
        rect,
        FIELD_RADIUS,
        label,
        Some(icon),
        variant,
        disabled,
        &theme.tokens,
    );
}

pub(super) fn kind_label(kind: ProviderKindChoice) -> &'static str {
    match kind {
        ProviderKindChoice::Anthropic => "Anthropic",
        ProviderKindChoice::OpenAi => "OpenAI",
        ProviderKindChoice::Ollama => "Ollama",
    }
}

pub(super) fn status_label(status: &ProviderModelsStatus) -> &'static str {
    match status {
        ProviderModelsStatus::Idle => "",
        ProviderModelsStatus::Saving { .. } => "正在保存…",
        ProviderModelsStatus::Saved { .. } => "已保存，无需重启即可用于新会话。",
        ProviderModelsStatus::Removing { .. } => "正在删除…",
        ProviderModelsStatus::Removed { .. } => "Provider 已删除。",
        ProviderModelsStatus::Failed { .. } => "操作失败，请检查配置后重试。",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_characters_take_priority_over_a_stored_credential() {
        assert_eq!(secret_display(3, true), "•••");
        assert_eq!(secret_display(3, false), "•••");
    }

    #[test]
    fn pending_dots_are_capped_so_long_pastes_do_not_blow_out_the_field() {
        assert_eq!(
            secret_display(40, false),
            "•".repeat(MAX_PENDING_SECRET_DOTS)
        );
    }

    #[test]
    fn nothing_typed_falls_back_to_stored_or_placeholder() {
        assert_eq!(secret_display(0, true), "••••••••");
        assert_eq!(secret_display(0, false), "输入密钥");
    }
}
