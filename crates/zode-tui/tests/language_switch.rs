use ratatui::{backend::TestBackend, Terminal};
use zode_tui::theme::ThemeStore;
use zode_tui::ui::chat::{ChatRenderMeta, ChatView};

fn screen_text(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn empty_state_retranslates_after_runtime_language_switch() {
    zode_core::i18n::set_language(zode_core::i18n::Lang::En);

    let theme = ThemeStore::with_builtins().resolve(Some("minimal"));
    let mut chat = ChatView::new();
    let mut terminal = Terminal::new(TestBackend::new(100, 12)).unwrap();
    let meta = ChatRenderMeta {
        theme_name: &theme.name,
        model: "deepseek-v4-pro",
        cwd: std::path::Path::new("/tmp/zode"),
    };

    terminal
        .draw(|frame| chat.render(frame, frame.area(), &theme, meta))
        .unwrap();
    let english = screen_text(&terminal);
    assert!(english.contains("workbench"));
    assert!(english.contains("terminal coding agent"));

    zode_core::i18n::set_language(zode_core::i18n::Lang::Zh);
    // Keep the ChatView (and therefore its render caches) but use a fresh
    // backend so double-width CJK cells cannot retain artifacts from the
    // previous English frame in TestBackend.
    let mut translated_terminal = Terminal::new(TestBackend::new(100, 12)).unwrap();
    translated_terminal
        .draw(|frame| chat.render(frame, frame.area(), &theme, meta))
        .unwrap();
    let chinese = screen_text(&translated_terminal);
    let compact_chinese: String = chinese.chars().filter(|c| !c.is_whitespace()).collect();
    assert!(
        compact_chinese.contains("工作台"),
        "second frame:\n{chinese}"
    );
    assert!(
        compact_chinese.contains("终端编程代理"),
        "second frame:\n{chinese}"
    );
    assert!(!chinese.contains("workbench"), "second frame:\n{chinese}");

    zode_core::i18n::set_language(zode_core::i18n::Lang::En);
}
