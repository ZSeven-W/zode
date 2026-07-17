use zode_app_model::{
    demo_state, reduce_presentation_command, AppCommand, IntegrationScope, IntegrationsTab,
    PresentationCommandOutcome, ShellRoute,
};

#[test]
fn integration_scroll_is_route_local_finite_and_resets_when_catalog_content_changes() {
    let mut state = demo_state();
    assert_eq!(
        reduce_presentation_command(
            &mut state,
            AppCommand::SetIntegrationsScroll { offset: 120.0 },
        ),
        PresentationCommandOutcome::Ignored,
    );

    reduce_presentation_command(
        &mut state,
        AppCommand::Navigate(ShellRoute::Integrations(IntegrationsTab::Plugins)),
    );
    assert_eq!(
        reduce_presentation_command(
            &mut state,
            AppCommand::SetIntegrationsScroll { offset: f32::NAN },
        ),
        PresentationCommandOutcome::Ignored,
    );
    assert_eq!(
        reduce_presentation_command(
            &mut state,
            AppCommand::SetIntegrationsScroll { offset: -12.0 },
        ),
        PresentationCommandOutcome::Applied,
    );
    assert_eq!(state.integration_scroll_offset, 0.0);

    reduce_presentation_command(
        &mut state,
        AppCommand::SetIntegrationsScroll { offset: 128.5 },
    );
    assert_eq!(state.integration_scroll_offset, 128.5);
    reduce_presentation_command(
        &mut state,
        AppCommand::SetIntegrationSearch("review".into()),
    );
    assert_eq!(state.integration_scroll_offset, 0.0);

    reduce_presentation_command(
        &mut state,
        AppCommand::SetIntegrationsScroll { offset: 96.0 },
    );
    reduce_presentation_command(
        &mut state,
        AppCommand::SetIntegrationScope(IntegrationScope::Public),
    );
    assert_eq!(state.integration_scroll_offset, 0.0);

    reduce_presentation_command(
        &mut state,
        AppCommand::SetIntegrationsScroll { offset: 64.0 },
    );
    reduce_presentation_command(
        &mut state,
        AppCommand::SelectIntegrationsTab(IntegrationsTab::Skills),
    );
    assert_eq!(state.integration_scroll_offset, 0.0);
}
