//! External teammate sessions. Resumable CLIs keep their conversation across
//! assignments; one-shot CLIs remain useful as stateless teammates and start a
//! fresh process for every send. Trust follows the two-phase grant from Phase
//! A. A fingerprint change discards any captured session id.

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
    pub fn new(def: ExternalAgentDef) -> Result<Self, TeamError> {
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

        let mut planned_session_id = None;
        let (prompt, extra_args) = match &self.session_id {
            None => {
                let p = match preamble {
                    Some(pre) => format!("{pre}\n\n{message}"),
                    None => message.to_string(),
                };
                let args = if let Some(template) = &self.def.capability.new_session_args {
                    let id = uuid::Uuid::new_v4().to_string();
                    planned_session_id = Some(id.clone());
                    template
                        .iter()
                        .map(|arg| {
                            if arg == "{session_id}" {
                                id.clone()
                            } else {
                                arg.clone()
                            }
                        })
                        .collect()
                } else {
                    vec![]
                };
                (p, args)
            }
            Some(id) => {
                let args = if let Some(template) = &self.def.capability.resume_args {
                    template
                        .iter()
                        .map(|arg| {
                            if arg == "{session_id}" {
                                id.clone()
                            } else {
                                arg.clone()
                            }
                        })
                        .collect()
                } else {
                    vec![
                        self.def.capability.resume_flag.clone().unwrap_or_default(),
                        id.clone(),
                    ]
                };
                (message.to_string(), args)
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

        // Capture the session id on the first successful run when this profile
        // promises resume support. Stateless teammates intentionally keep it
        // as None, so the next send starts a fresh run with the preamble.
        let supports_resume =
            self.def.capability.resume_flag.is_some() || self.def.capability.resume_args.is_some();
        if supports_resume && self.session_id.is_none() {
            match outcome.result.session_id.clone().or(planned_session_id) {
                Some(id) => self.session_id = Some(id),
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
                resume_args: None,
                new_session_args: None,
                effective_sandbox: EffectiveSandbox::Unrestricted,
                version_requirement: None,
                session_id_source: Some("/session_id".into()),
                text_source: None,
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
    fn non_resumable_profile_becomes_a_stateless_teammate() {
        let mut def = resumable_def("fake-claude.sh");
        def.capability.resume_flag = None;
        def.capability.resume_args = None;
        assert!(ExternalSession::new(def).is_ok());
    }

    #[test]
    fn resume_args_make_a_profile_resumable() {
        let mut def = resumable_def("fake-claude.sh");
        def.capability.resume_flag = None;
        def.capability.resume_args = Some(vec!["--session".into(), "{session_id}".into()]);
        assert!(ExternalSession::new(def).is_ok());
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn host_selected_session_id_makes_text_cli_resumable() {
        let dir = tempfile::tempdir().unwrap();
        let mut def = resumable_def("fake-resume.sh");
        def.capability.output_protocol = OutputProtocol::Text;
        def.capability.resume_flag = None;
        def.capability.new_session_args = Some(vec!["--session-id".into(), "{session_id}".into()]);
        def.capability.resume_args = Some(vec!["--resume".into(), "{session_id}".into()]);
        let grants = granted(&def, dir.path());
        let mut session = ExternalSession::new(def).unwrap();

        session
            .send(
                "task one",
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
        let id = session
            .session_id
            .clone()
            .expect("host-selected session id");
        assert!(uuid::Uuid::parse_str(&id).is_ok());

        let resumed = session
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
        assert!(resumed.result.text.contains("resumed-ok"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn stateless_teammate_starts_a_fresh_run_on_each_send() {
        let dir = tempfile::tempdir().unwrap();
        let mut def = resumable_def("fake-resume.sh");
        def.capability.resume_flag = None;
        def.capability.resume_args = None;
        let grants = granted(&def, dir.path());
        let mut session = ExternalSession::new(def).unwrap();

        for message in ["task one", "task two"] {
            let out = session
                .send(
                    message,
                    Some("you are a stateless teammate"),
                    &grants,
                    dir.path(),
                    Duration::from_secs(30),
                    None,
                    |_| {},
                    AbortController::new(),
                )
                .await
                .unwrap();
            assert_eq!(out.result.text, "first-run done");
            assert!(session.session_id.is_none());
        }
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn first_send_captures_session_id_then_resumes() {
        let dir = tempfile::tempdir().unwrap();
        let mut def = resumable_def("fake-resume.sh");
        def.capability.resume_flag = None;
        def.capability.resume_args = Some(vec!["--resume".into(), "{session_id}".into()]);
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
