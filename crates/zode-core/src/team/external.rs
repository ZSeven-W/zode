//! External teammate sessions: a resumable agent CLI. A profile without
//! resume capability cannot become a teammate. Trust follows the two-phase
//! grant from Phase A: hire only approves (PendingGrant); the first send
//! re-hashes, probes `--version`, and promotes to Granted while capturing the
//! session id. A fingerprint change after a session exists discards the old
//! session id (no resume onto a changed binary).

use std::sync::Arc;
use std::time::Duration;

use agent::abort::AbortController;
use agent::file_cache::FileStateCache;

use super::TeamError;
use crate::external_agents::runner::{run_external, RunOutcome, RunSpec};
use crate::external_agents::{
    parser::ExtEvent, preapproval_fingerprint, ExternalAgentDef, GrantCheck, GrantStore,
};
use tokio::sync::Mutex as AsyncMutex;

#[derive(Debug)]
pub struct ExternalSession {
    pub def: ExternalAgentDef,
    pub session_id: Option<String>,
    run_lock: Arc<AsyncMutex<()>>,
}

impl ExternalSession {
    /// Fails if the profile cannot resume (one-shot-only CLIs can't be hired).
    pub fn new(def: ExternalAgentDef) -> Result<Self, TeamError> {
        if def.capability.resume_flag.is_none() {
            return Err(TeamError::Io(format!(
                "external agent '{}' does not support resume and cannot be a teammate",
                def.name
            )));
        }
        Ok(Self {
            def,
            session_id: None,
            run_lock: Arc::new(AsyncMutex::new(())),
        })
    }

    /// Whether the trust grant needs a (re)approval before the next send.
    pub fn needs_approval(&self, grants: &GrantStore, cwd: &std::path::Path) -> bool {
        match preapproval_fingerprint(&self.def, cwd) {
            Ok(fp) => grants.check(&self.def.name, &fp) == GrantCheck::Missing,
            Err(_) => true,
        }
    }

    /// Drive one send. First send (no session id) starts a new run with the
    /// preamble prepended; later sends resume. Concurrent send → Busy.
    #[allow(clippy::too_many_arguments)]
    pub async fn send(
        &mut self,
        message: &str,
        preamble: Option<&str>,
        grants: &GrantStore,
        cwd: &std::path::Path,
        timeout: Duration,
        file_cache: Option<Arc<FileStateCache>>,
        mut on_event: impl FnMut(ExtEvent) + Send,
        abort: AbortController,
    ) -> Result<RunOutcome, TeamError> {
        let Ok(_run) = self.run_lock.try_lock() else {
            return Err(TeamError::Busy {
                desc: "already handling a task".to_string(),
            });
        };

        // Trust re-check: fingerprint drift discards any resume context.
        let fp =
            preapproval_fingerprint(&self.def, cwd).map_err(|e| TeamError::Io(e.to_string()))?;
        match grants.check(&self.def.name, &fp) {
            GrantCheck::Missing => {
                self.session_id = None;
                return Err(TeamError::Io(format!(
                    "external teammate '{}' needs a trust (re)approval before running",
                    self.def.name
                )));
            }
            _ => {
                // re-hash before spawn (swap-window guard)
                let fp2 = preapproval_fingerprint(&self.def, cwd)
                    .map_err(|e| TeamError::Io(e.to_string()))?;
                if fp2.content_hash != fp.content_hash {
                    grants.revoke(&self.def.name);
                    self.session_id = None;
                    return Err(TeamError::Io(format!(
                        "executable for '{}' changed; grant revoked and session discarded",
                        self.def.name
                    )));
                }
            }
        }

        let (prompt, extra_args) = match &self.session_id {
            None => {
                let p = match preamble {
                    Some(pre) => format!("{pre}\n\n{message}"),
                    None => message.to_string(),
                };
                (p, vec![])
            }
            Some(id) => {
                let flag = self.def.capability.resume_flag.clone().unwrap_or_default();
                (message.to_string(), vec![flag, id.clone()])
            }
        };

        let spec = RunSpec {
            def: self.def.clone(),
            prompt,
            cwd: cwd.to_path_buf(),
            timeout,
            extra_args,
            file_cache,
        };
        let outcome = run_external(spec, &mut on_event, abort)
            .await
            .map_err(TeamError::Io)?;

        // Capture the session id on the first successful run.
        if self.session_id.is_none() {
            match &outcome.result.session_id {
                Some(id) => self.session_id = Some(id.clone()),
                None => {
                    return Err(TeamError::Io(format!(
                        "external teammate '{}' produced no session id; cannot become resumable",
                        self.def.name
                    )));
                }
            }
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::external_agents::capability::{
        EffectiveSandbox, OutputProtocol, ProfileCapability, PromptTransport,
    };
    use std::path::Path;

    fn resumable_def(script: &str) -> ExternalAgentDef {
        ExternalAgentDef {
            name: "fake-ext".into(),
            command: Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/extagent")
                .join(script),
            args: vec![],
            capability: ProfileCapability {
                prompt_transport: PromptTransport::Stdin,
                output_protocol: OutputProtocol::JsonlClaude,
                resume_flag: Some("--resume".into()),
                effective_sandbox: EffectiveSandbox::Unrestricted,
                version_requirement: None,
                session_id_source: Some("/session_id".into()),
            },
            auth_env: vec![],
            env_allow: vec![],
            trusted: true,
        }
    }

    fn granted(def: &ExternalAgentDef, cwd: &Path) -> GrantStore {
        let g = GrantStore::default();
        let mut fp = preapproval_fingerprint(def, cwd).unwrap();
        fp.version_output = Some("1.0".into());
        g.store_pending(&def.name, fp);
        g.promote(&def.name, "1.0".into());
        g
    }

    #[test]
    fn non_resumable_profile_cannot_become_teammate() {
        let mut def = resumable_def("fake-claude.sh");
        def.capability.resume_flag = None;
        assert!(ExternalSession::new(def).is_err());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn first_send_captures_session_id_then_resumes() {
        let dir = tempfile::tempdir().unwrap();
        let def = resumable_def("fake-resume.sh");
        let grants = granted(&def, dir.path());
        let mut s = ExternalSession::new(def).unwrap();
        let out1 = s
            .send(
                "task one",
                Some("you are t1"),
                &grants,
                dir.path(),
                Duration::from_secs(30),
                None,
                |_| {},
                AbortController::new(),
            )
            .await
            .unwrap();
        assert_eq!(s.session_id.as_deref(), Some("sess-A"));
        let _ = out1;
        let out2 = s
            .send(
                "task two",
                None,
                &grants,
                dir.path(),
                Duration::from_secs(30),
                None,
                |_| {},
                AbortController::new(),
            )
            .await
            .unwrap();
        assert!(
            out2.result.text.contains("resumed-ok"),
            "resume flag must reach the CLI: {}",
            out2.result.text
        );
    }
}
