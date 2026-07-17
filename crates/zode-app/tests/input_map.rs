use winit::{
    event::Ime,
    keyboard::{Key as WinitKey, ModifiersState, NamedKey},
};
use zode_app::event_map::{composer_outcome_command, map_ime, map_key};
use zode_app_model::AppCommand;
use zode_app_ui::{ComposerOutcome, ImeEvent, Key, KeyEvent, Modifiers};
use zode_node_protocol::UserContent;

#[test]
fn named_and_character_keys_map_to_platform_neutral_events() {
    assert_eq!(
        map_key(
            &WinitKey::Named(NamedKey::Enter),
            ModifiersState::SHIFT,
            true,
        ),
        Some(KeyEvent {
            key: Key::Enter,
            modifiers: Modifiers::SHIFT,
            pressed: true,
        }),
    );
    assert_eq!(
        map_key(
            &WinitKey::Character("你".into()),
            ModifiersState::empty(),
            true,
        ),
        Some(KeyEvent {
            key: Key::Character("你".into()),
            modifiers: Modifiers::NONE,
            pressed: true,
        }),
    );
    assert_eq!(
        map_key(
            &WinitKey::Named(NamedKey::F12),
            ModifiersState::empty(),
            true,
        ),
        None,
    );
}

#[test]
fn ime_preedit_uses_the_cursor_end_and_preserves_lifecycle() {
    assert_eq!(map_ime(&Ime::Enabled), ImeEvent::Start);
    assert_eq!(
        map_ime(&Ime::Preedit("拼音".into(), Some((0, "拼".len())))),
        ImeEvent::Update {
            text: "拼音".into(),
            cursor: Some("拼".len()),
        },
    );
    assert_eq!(
        map_ime(&Ime::Commit("中文".into())),
        ImeEvent::Commit("中文".into()),
    );
    assert_eq!(map_ime(&Ime::Disabled), ImeEvent::End);
}

#[test]
fn composer_outcomes_map_to_controller_commands() {
    assert_eq!(
        composer_outcome_command(ComposerOutcome::Send("hello".into())),
        Some(AppCommand::Submit(vec![UserContent::Text {
            text: "hello".into(),
        }])),
    );
    assert_eq!(
        composer_outcome_command(ComposerOutcome::Steer("redirect".into())),
        Some(AppCommand::Steer(vec![UserContent::Text {
            text: "redirect".into(),
        }])),
    );
    assert_eq!(
        composer_outcome_command(ComposerOutcome::Stop),
        Some(AppCommand::Interrupt),
    );
    assert_eq!(composer_outcome_command(ComposerOutcome::Edited), None,);
}
