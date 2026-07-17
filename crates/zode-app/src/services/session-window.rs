use std::process::{Command, Stdio};

use zode_node_protocol::SessionLocator;

use super::ServiceError;

/// Opens an existing task in a second Zode process. The trait keeps process
/// creation replaceable for deterministic controller tests.
pub trait SessionWindowService: Send + Sync {
    fn open_session(&self, session: &SessionLocator) -> Result<(), ServiceError>;
}

#[derive(Default)]
pub struct NativeSessionWindowService;

impl SessionWindowService for NativeSessionWindowService {
    fn open_session(&self, session: &SessionLocator) -> Result<(), ServiceError> {
        let executable = std::env::current_exe()?;
        Command::new(executable)
            .arg("--session")
            .arg(&session.session_id)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(())
    }
}
