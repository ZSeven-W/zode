#[test]
fn workspace_shell_paint_borrows_the_workspace_snapshot() {
    let source = include_str!("../src/widgets/workspace_shell.rs");
    let paint_body = source
        .split_once("fn paint_snapshot_content(")
        .expect("workspace shell paint entry point")
        .1
        .split_once("\n}\n\nstruct ConversationPaintContext")
        .expect("workspace shell paint body")
        .0;

    assert!(
        !paint_body.contains("snapshot.clone()"),
        "painting must borrow WorkspaceSnapshot instead of cloning its interaction tree"
    );
}
