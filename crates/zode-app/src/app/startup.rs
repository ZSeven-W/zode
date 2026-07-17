use std::sync::Arc;

use winit::event_loop::{ControlFlow, EventLoop};
use zode_app_model::{reduce_navigation_command, AppCommand, NavigationOutcome};
use zode_app_runtime::{path_to_workspace_uri, AppStateStore, LocalAppRuntime};
use zode_core::{bootstrap::AppBootstrap, config::ConfigManager};
use zode_node_protocol::AgentEndpoint;

use super::DesktopApp;
use crate::{bootstrap_state::load_initial_state, window_state::AppWake};

pub fn run_demo() -> Result<(), Box<dyn std::error::Error>> {
    run_demo_with_session(None)
}

pub fn run_demo_for_session(session_id: String) -> Result<(), Box<dyn std::error::Error>> {
    run_demo_with_session(Some(session_id))
}

fn run_demo_with_session(session_id: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let tokio_runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let cwd = std::env::current_dir()?;
    let startup_workspace = path_to_workspace_uri(&cwd)?;
    let bootstrap = tokio_runtime.block_on(AppBootstrap::new(cwd).resolve())?;
    let provider_setup_required = bootstrap.needs_setup;
    let config_dir = ConfigManager::config_dir()?;
    let projectless_workspace = config_dir.join("task-workspaces");
    ensure_private_task_root(&projectless_workspace)?;
    let projectless_workspace_root = path_to_workspace_uri(&projectless_workspace)?;
    let persisted = match AppStateStore::new(&config_dir).load() {
        Ok(state) => Some(state),
        Err(error) => {
            eprintln!("zode-app: task ownership state could not be loaded: {error}");
            None
        }
    };
    let runtime = {
        let _guard = tokio_runtime.enter();
        LocalAppRuntime::new(config_dir, bootstrap, 256)?
    };
    let endpoint: Arc<dyn AgentEndpoint> = runtime.endpoint();
    let browser_session = runtime.browser_session();
    let mut state = tokio_runtime.block_on(load_initial_state(
        endpoint.as_ref(),
        runtime.capabilities().clone(),
        startup_workspace,
        projectless_workspace_root,
        persisted.as_ref(),
    ))?;
    state.provider_setup_required = provider_setup_required;

    let event_loop = EventLoop::<AppWake>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let proxy = event_loop.create_proxy();
    let mut app = DesktopApp::new(state, proxy);
    if let Some(session_id) = session_id {
        let session = app
            .app_state
            .threads
            .iter()
            .find(|thread| thread.session.session_id == session_id)
            .map(|thread| thread.session.clone())
            .ok_or_else(|| format!("task session not found: {session_id}"))?;
        if reduce_navigation_command(&mut app.app_state, AppCommand::SelectSession(session))
            != NavigationOutcome::Applied
        {
            return Err(format!("task session is unavailable: {session_id}").into());
        }
        app.rebuild_frame_snapshot();
    }
    let _guard = tokio_runtime.enter();
    app.attach_endpoint(endpoint);
    app.attach_browser_session(browser_session);
    event_loop.run_app(&mut app)?;
    Ok(())
}

fn ensure_private_task_root(path: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
