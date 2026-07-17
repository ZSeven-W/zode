use zode_app_model::{demo_state, reduce_settings_command, AppCommand, SettingsCommandOutcome};
use zode_app_ui::SettingsPanel;
use zode_node_protocol::WorkspaceUri;

#[test]
fn project_permissions_are_sorted_and_each_row_revokes_only_its_tool() {
    let workspace = WorkspaceUri::new("file:///repo/zode").unwrap();
    let mut state = demo_state();
    assert_eq!(
        reduce_settings_command(
            &mut state,
            AppCommand::SetProjectPermissions {
                workspace_uri: workspace.clone(),
                tools: vec!["FileEdit".into(), "Bash".into(), "Bash".into()],
            },
        ),
        SettingsCommandOutcome::Applied,
    );

    let rows = SettingsPanel::permission_rows(&state, &workspace);
    assert_eq!(
        rows.iter().map(|row| row.tool.as_str()).collect::<Vec<_>>(),
        vec!["Bash", "FileEdit"],
    );
    assert_eq!(
        rows[0].revoke_command,
        AppCommand::RevokeProjectPermission {
            workspace_uri: workspace,
            tool: "Bash".into(),
        },
    );
}

#[test]
fn unknown_workspace_has_no_stale_permission_rows() {
    let state = demo_state();
    let workspace = WorkspaceUri::new("file:///repo/missing").unwrap();
    assert!(SettingsPanel::permission_rows(&state, &workspace).is_empty());
}
