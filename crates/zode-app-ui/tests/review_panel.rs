use zode_app_model::AppCommand;
use zode_app_ui::{ReviewDraft, ReviewLineKind, ReviewPanel, ReviewSelection};
use zode_node_protocol::WorkspaceUri;

const UNIFIED: &str =
    "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,3 @@\n one\n+two\n three\n";

#[test]
fn unified_hunks_project_line_numbers_and_change_kinds() {
    let lines = ReviewPanel::parse_unified(UNIFIED);
    assert_eq!(lines.len(), 4);
    assert_eq!(lines[0].kind, ReviewLineKind::Hunk);
    assert_eq!(lines[1].old_line, Some(1));
    assert_eq!(lines[1].new_line, Some(1));
    assert_eq!(lines[1].kind, ReviewLineKind::Context);
    assert_eq!(lines[2].old_line, None);
    assert_eq!(lines[2].new_line, Some(2));
    assert_eq!(lines[2].kind, ReviewLineKind::Addition);
    assert_eq!(lines[3].old_line, Some(2));
    assert_eq!(lines[3].new_line, Some(3));
}

#[test]
fn long_hunk_virtualization_keeps_one_overscan_line() {
    assert_eq!(
        ReviewPanel::visible_line_range(100, 200.0, 60.0, 20.0),
        9..13,
    );
}

#[test]
fn review_draft_tracks_selection_and_inline_comment_without_touching_git() {
    let mut draft = ReviewDraft::default();
    draft.select(ReviewSelection {
        path: "a.txt".into(),
        start_line: 2,
        end_line: 4,
    });
    draft.set_comment("Please keep this branch".into());

    assert_eq!(draft.selection().unwrap().start_line, 2);
    assert_eq!(draft.comment(), "Please keep this branch");
}

#[test]
fn open_file_action_routes_through_the_host_command() {
    let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
    assert_eq!(
        ReviewPanel::open_file_command(workspace.clone(), "src/main.rs"),
        AppCommand::OpenWorkspaceFile {
            workspace_uri: workspace,
            relative_path: "src/main.rs".into(),
        },
    );
}
