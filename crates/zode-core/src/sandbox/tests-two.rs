    #[test]
    fn protected_metadata_dirs_stay_read_only_in_workspace_write() {
        let p = cfg(SandboxMode::WorkspaceWrite, false).macos_profile();
        // cwd is writable...
        assert!(p.contains("(allow file-write* (subpath \"/work/proj\"))"));
        // ...but .git and .zode under it are carved back out (deny AFTER allow).
        assert!(
            p.contains("(deny file-write* (subpath \"/work/proj/.git\"))"),
            ".git protected: {p}"
        );
        assert!(
            p.contains("(deny file-write* (subpath \"/work/proj/.zode\"))"),
            ".zode protected: {p}"
        );
        let allow_at = p
            .find("(allow file-write* (subpath \"/work/proj\"))")
            .unwrap();
        let deny_at = p
            .find("(deny file-write* (subpath \"/work/proj/.git\"))")
            .unwrap();
        assert!(
            deny_at > allow_at,
            "deny must follow allow for last-match-wins"
        );
    }

    #[test]
    fn with_cwd_rebases_protected_paths_to_the_new_dir() {
        let c = cfg(SandboxMode::WorkspaceWrite, false); // cwd /work/proj
        assert!(c
            .protected_paths()
            .iter()
            .any(|p| p == Path::new("/work/proj/.git")));
        // A resumed tab in another repo must protect THAT repo's metadata.
        let rebased = c.with_cwd(Path::new("/elsewhere/repo"));
        assert!(rebased
            .protected_paths()
            .iter()
            .any(|p| p == Path::new("/elsewhere/repo/.zode")));
        assert!(
            !rebased
                .protected_paths()
                .iter()
                .any(|p| p.starts_with("/work/proj")),
            "old cwd no longer protected after rebase"
        );
    }

    #[test]
    fn write_scope_summary_names_tmp_only_when_writable() {
        // Default policy: /tmp IS writable, so user-facing text must say so —
        // claiming "confined to the workspace" reads as a broken sandbox the
        // moment a user tests it with /tmp.
        let s = cfg(SandboxMode::WorkspaceWrite, false).write_scope_summary();
        assert!(s.contains("workspace"), "{s}");
        assert!(
            s.contains("/tmp"),
            "default /tmp writability must be named: {s}"
        );
        // Excluded → not advertised.
        let s = cfg(SandboxMode::WorkspaceWrite, false)
            .with_temp_policy(true, true)
            .write_scope_summary();
        assert!(
            !s.contains("/tmp"),
            "excluded /tmp must not be advertised: {s}"
        );
        // Extra writable roots are surfaced too.
        let mut c = cfg(SandboxMode::WorkspaceWrite, false);
        c.writable_roots = vec![PathBuf::from("/data/cache")];
        assert!(c.write_scope_summary().contains("/data/cache"));
    }

    #[test]
    fn bash_tool_description_names_the_real_write_scope() {
        let rec = Arc::new(RecordingTool::default());
        let dir = tempfile::tempdir().unwrap();
        let Ok(config) = SandboxConfig::for_current_os(dir.path()) else {
            return; // unsupported OS
        };
        let tool = SandboxedBashTool::new(rec, config, allow_gate());
        assert!(
            tool.description().contains("/tmp"),
            "description must not claim workspace-only while /tmp is writable: {}",
            tool.description()
        );
    }

    #[test]
    fn resolve_with_settings_honors_config_roots_and_temp_policy() {
        let dir = tempfile::tempdir().unwrap();
        let extra = tempfile::tempdir().unwrap();
        let settings = crate::config::SandboxSettings {
            writable_roots: vec![extra.path().display().to_string()],
            exclude_slash_tmp: Some(true),
            exclude_tmpdir_env_var: Some(true),
            restrict_reads: Some(true),
            ..Default::default()
        };
        let c = match resolve_with_settings(
            dir.path(),
            &settings,
            SandboxMode::WorkspaceWrite,
            false,
        ) {
            Ok(c) => c,
            Err(_) => return, // unsupported host / missing backend
        };
        let canon_extra = canonical(extra.path());
        assert!(
            c.writable_roots().iter().any(|r| r == &canon_extra),
            "config writableRoots must survive a runtime toggle: {:?}",
            c.writable_roots()
        );
        assert!(
            !c.writable_dirs().iter().any(|d| d == "/tmp"),
            "excludeSlashTmp must survive a runtime toggle: {:?}",
            c.writable_dirs()
        );
        assert!(c.restrict_reads(), "restrictReads must be applied");
        assert_eq!(c.mode(), SandboxMode::WorkspaceWrite);
        assert!(!c.allow_network());
    }

    #[test]
    fn exclude_slash_tmp_drops_tmp_from_writable_roots() {
        let with_tmp = cfg(SandboxMode::WorkspaceWrite, false);
        assert!(with_tmp.writable_dirs().iter().any(|d| d == "/tmp"));
        let without = with_tmp.with_temp_policy(true, true);
        assert!(
            !without.writable_dirs().iter().any(|d| d == "/tmp"),
            "exclude_slash_tmp drops /tmp: {:?}",
            without.writable_dirs()
        );
    }

    /// Tool that fails (sandbox-denied) for the wrapped command but succeeds
    /// for the raw one; records every command it received.
    #[derive(Debug, Default)]
    struct DenyThenRecord {
        calls: std::sync::Mutex<Vec<String>>,
    }
    #[async_trait]
    impl Tool for DenyThenRecord {
        fn name(&self) -> &str {
            "Bash"
        }
        fn description(&self) -> &str {
            "x"
        }
        fn input_schema(&self) -> serde_json::Value {
            json!({"type": "object", "properties": {"command": {"type": "string"}}})
        }
        fn safety_class(&self) -> SafetyClass {
            SafetyClass::Mutating
        }
        async fn call(
            &self,
            _ctx: &ToolUseContext,
            input: serde_json::Value,
        ) -> Result<serde_json::Value, AgentError> {
            let cmd = input["command"].as_str().unwrap_or("").to_string();
            let sandboxed = cmd.contains("sandbox-exec") || cmd.contains("bwrap");
            self.calls.lock().unwrap().push(cmd);
            if sandboxed {
                Ok(json!({"exit_code": 1, "stderr": "Operation not permitted"}))
            } else {
                Ok(json!({"exit_code": 0, "stdout": "ok"}))
            }
        }
    }

    #[tokio::test]
    async fn sandbox_denial_offers_escalation_and_reruns_raw() {
        let inner = Arc::new(DenyThenRecord::default());
        let cfg = SandboxConfig::for_current_os(Path::new("/tmp")).unwrap();
        let gate = PromptGate::new(Approval::AllowOnce);
        let tool = SandboxedBashTool::new(inner.clone(), cfg, gate.clone());
        let ctx = ToolUseContext::new(std::env::temp_dir());

        let out = tool
            .call(&ctx, json!({"command": "echo hi"}))
            .await
            .unwrap();
        // The escalation (user-approved) re-ran the raw command, which succeeded.
        assert_eq!(out["exit_code"], 0, "escalated raw run returned success");
        let calls = inner.calls.lock().unwrap();
        assert_eq!(
            calls.len(),
            2,
            "ran sandboxed, then escalated raw: {calls:?}"
        );
        assert!(
            calls[0].contains("sandbox-exec") || calls[0].contains("bwrap"),
            "first run was sandboxed"
        );
        assert_eq!(calls[1], "echo hi", "re-run uses the original command");
        assert_eq!(gate.asked.lock().unwrap().len(), 1, "user was asked");
    }

    #[tokio::test]
    async fn sandbox_denial_does_not_escalate_under_a_bypass_gate() {
        // yolo / bypass: an auto-answering gate must NOT "approve" the
        // escalation — the failed command stays failed and stays sandboxed.
        let inner = Arc::new(DenyThenRecord::default());
        let cfg = SandboxConfig::for_current_os(Path::new("/tmp")).unwrap();
        let tool = SandboxedBashTool::new(inner.clone(), cfg, allow_gate());
        let ctx = ToolUseContext::new(std::env::temp_dir());

        let out = tool
            .call(&ctx, json!({"command": "echo hi"}))
            .await
            .unwrap();
        assert_eq!(out["exit_code"], 1, "sandboxed failure surfaces as-is");
        let calls = inner.calls.lock().unwrap();
        assert_eq!(calls.len(), 1, "no silent unsandboxed re-run: {calls:?}");
    }

    #[tokio::test]
    async fn non_sandbox_failure_does_not_escalate() {
        // A plain failure (no sandbox signature) must not trigger escalation.
        #[derive(Debug, Default)]
        struct AlwaysFail(std::sync::Mutex<usize>);
        #[async_trait]
        impl Tool for AlwaysFail {
            fn name(&self) -> &str {
                "Bash"
            }
            fn description(&self) -> &str {
                "x"
            }
            fn input_schema(&self) -> serde_json::Value {
                json!({"type": "object", "properties": {"command": {"type": "string"}}})
            }
            fn safety_class(&self) -> SafetyClass {
                SafetyClass::Mutating
            }
            async fn call(
                &self,
                _ctx: &ToolUseContext,
                _input: serde_json::Value,
            ) -> Result<serde_json::Value, AgentError> {
                *self.0.lock().unwrap() += 1;
                Ok(json!({"exit_code": 1, "stderr": "command not found: frobnicate"}))
            }
        }
        let inner = Arc::new(AlwaysFail::default());
        let cfg = SandboxConfig::for_current_os(Path::new("/tmp")).unwrap();
        // Interactive gate, so a false escalation offer WOULD be visible here.
        let gate = PromptGate::new(Approval::AllowOnce);
        let tool = SandboxedBashTool::new(inner.clone(), cfg, gate.clone());
        let ctx = ToolUseContext::new(std::env::temp_dir());
        tool.call(&ctx, json!({"command": "frobnicate"}))
            .await
            .unwrap();
        assert_eq!(
            *inner.0.lock().unwrap(),
            1,
            "ran once, no escalation re-run"
        );
        assert!(
            gate.asked.lock().unwrap().is_empty(),
            "no escalation prompt for a non-sandbox failure"
        );
    }

    #[test]
    fn denial_detection_matches_sandbox_signs_but_rejects_exec_errors() {
        // Real sandbox denials.
        assert!(looks_like_sandbox_denial(
            &json!({"exit_code": 1, "stderr": "touch: /x: Read-only file system"})
        ));
        assert!(looks_like_sandbox_denial(
            &json!({"exit_code": 1, "stderr": "curl: (6) Could not resolve host: example.com"})
        ));
        // exit 0 is never a denial.
        assert!(!looks_like_sandbox_denial(
            &json!({"exit_code": 0, "stderr": "sandbox"})
        ));
        // 126/127 are exec failures — NOT sandbox denials — even though 126
        // prints "permission denied" (which would otherwise keyword-match).
        assert!(!looks_like_sandbox_denial(
            &json!({"exit_code": 126, "stderr": "bash: ./run: Permission denied"})
        ));
        assert!(!looks_like_sandbox_denial(
            &json!({"exit_code": 127, "stderr": "bash: frobnicate: command not found"})
        ));
    }

    #[tokio::test]
    async fn sandboxed_fs_sink_writes_inside_but_kernel_blocks_protected_git() {
        use agent_tools_code::FsSink;
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path();
        let config = match SandboxConfig::new(cwd, SandboxMode::WorkspaceWrite, false, &[]) {
            Ok(c) => c,
            Err(_) => return, // unsupported OS — nothing to prove here
        };
        if config.os == SandboxOs::Linux && !binary_on_path("bwrap") {
            return; // backend unavailable
        }
        // .git must already exist so the denied write fails on the SANDBOX, not
        // on a missing parent dir.
        std::fs::create_dir_all(cwd.join(".git")).unwrap();
        let sink = SandboxedFsSink::new(config);

        // A normal write inside the workspace goes through the sandboxed `cat`.
        let inside = canonical(cwd).join("ok.txt");
        sink.write_file(&inside, b"hello")
            .await
            .expect("write inside the workspace must succeed");
        assert_eq!(std::fs::read_to_string(&inside).unwrap(), "hello");

        // A write into .git is blocked by the KERNEL (deny-after-allow in the
        // profile), not by zode — proving the file-tool write is OS-enforced.
        let protected = canonical(cwd).join(".git").join("HACK");
        let err = sink
            .write_file(&protected, b"x")
            .await
            .expect_err("kernel must deny the write into .git");
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied, "{err}");
        assert!(
            !protected.exists(),
            "the protected file must not have been created"
        );
    }

    #[test]
    fn canary_path_is_outside_writable_roots_or_absent() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        // Workspace == home → every home path is writable; nothing provable.
        let mut at_home = cfg(SandboxMode::WorkspaceWrite, false);
        at_home.cwd = canonical(&home);
        assert!(
            at_home.canary_path().is_none(),
            "home-as-workspace must skip the canary"
        );
        // Normal workspace → canary lives in home, outside the writable roots.
        let c = cfg(SandboxMode::WorkspaceWrite, false); // cwd /work/proj
        if let Some(p) = c.canary_path() {
            assert!(p.starts_with(canonical(&home)), "{}", p.display());
            assert!(
                !c.writable_dirs()
                    .iter()
                    .any(|d| p.starts_with(Path::new(d))),
                "canary must be outside every writable root: {}",
                p.display()
            );
        }
    }

    #[tokio::test]
    async fn verify_proves_enforcement_on_a_working_host() {
        let dir = tempfile::tempdir().unwrap();
        let Ok(config) = SandboxConfig::new(dir.path(), SandboxMode::WorkspaceWrite, false, &[])
        else {
            return; // unsupported OS
        };
        if config.os == SandboxOs::Linux && !binary_on_path("bwrap") {
            return; // backend unavailable
        }
        match config.verify().await {
            // Working host: enforcement proven.
            Ok(()) => {}
            // A CI container may run bwrap without user namespaces — verify
            // must then FAIL (closed) with the actionable backend message,
            // never claim enforcement.
            Err(e) => {
                let msg = e.to_string();
                assert!(msg.contains("sandbox"), "{msg}");
            }
        }
    }

    #[tokio::test]
    async fn network_canary_detects_traffic_when_network_allowed() {
        // True-positive path: with network ALLOWED, the sandboxed client must
        // reach the in-process listener — proving the canary detects traffic
        // whenever the OS lets it through (the leak case verify() rejects).
        let dir = tempfile::tempdir().unwrap();
        let Ok(config) = SandboxConfig::new(dir.path(), SandboxMode::WorkspaceWrite, true, &[])
        else {
            return; // unsupported OS
        };
        if config.os == SandboxOs::Linux && !binary_on_path("bwrap") {
            return; // backend unavailable
        }
        if !config.probe("true").await.unwrap_or(false) {
            return; // backend present but can't run (e.g. CI without userns)
        }
        match config.network_canary_leaked().await {
            Ok(Some(leaked)) => assert!(leaked, "allowed network must reach the listener"),
            Ok(None) => {} // no curl on this host — nothing to prove
            Err(e) => panic!("network canary errored: {e}"),
        }
    }

    #[test]
    fn resolve_disabled_is_ok_none() {
        // Explicitly disabled → Ok(None), the ONLY way to get no sandbox.
        assert!(matches!(
            resolve(
                Path::new("/tmp"),
                false,
                SandboxMode::WorkspaceWrite,
                false,
                &[],
                false,
                false
            ),
            Ok(None)
        ));
    }

    #[test]
    fn backend_available_fails_closed_on_linux_without_bwrap() {
        // Deterministic (no dependence on the host's real backend): Linux with
        // no bwrap is the fail-closed Err path; with bwrap, or macOS, it's Ok.
        assert!(backend_available(SandboxOs::Linux, true).is_ok());
        assert!(
            backend_available(SandboxOs::Linux, false).is_err(),
            "Linux without bwrap must fail closed, not run unconfined"
        );
        assert!(
            backend_available(SandboxOs::MacOs, false).is_ok(),
            "macOS has Seatbelt built in"
        );
    }

    #[test]
    fn resolve_enabled_never_resolves_to_silent_none() {
        // Whatever the host, a REQUESTED sandbox is never a silent Ok(None):
        // Ok(Some) where a backend exists, else a fail-closed Err.
        let r = resolve(
            Path::new("/tmp"),
            true,
            SandboxMode::WorkspaceWrite,
            false,
            &[],
            false,
            false,
        );
        assert!(
            !matches!(r, Ok(None)),
            "a requested sandbox must never resolve to silent no-isolation"
        );
    }

    #[test]
    fn sandbox_unavailable_message_is_actionable() {
        let s = sandbox_unavailable("`bwrap` was not found").to_string();
        assert!(s.contains("--no-sandbox"), "{s}");
        assert!(s.contains("bubblewrap") || s.contains("bwrap"), "{s}");
    }

    #[test]
    fn shell_join_quotes_unsafe_args() {
        assert_eq!(shell_join(&["echo".into(), "hi".into()]), "echo hi");
        let joined = shell_join(&["sh".into(), "-c".into(), "rm -rf x".into()]);
        assert!(joined.contains("'rm -rf x'"));
    }

    #[test]
    fn supported_platform_check() {
        let r = SandboxConfig::for_current_os(Path::new("/x"));
        if cfg!(any(target_os = "macos", target_os = "linux")) {
            assert!(r.is_ok());
        } else {
            assert!(r.is_err());
        }
    }

    #[test]
    fn for_current_os_canonicalizes_cwd() {
        let dir = tempfile::tempdir().unwrap();
        let real = std::fs::canonicalize(dir.path()).unwrap();
        if let Ok(cfg) = SandboxConfig::for_current_os(dir.path()) {
            assert_eq!(cfg.cwd, real);
        }
    }

#[test]
fn shared_resolver_applies_profile_overlay_overrides_and_extras() {
    let dir = tempfile::tempdir().unwrap();
    let mut settings = crate::config::SandboxSettings {
        enabled: Some(true),
        network: Some(false),
        windows_tier: Some("tier2".into()),
        ..Default::default()
    };
    settings.profiles.insert(
        "custom".into(),
        crate::config::SandboxProfile {
            network: Some(true),
            ..Default::default()
        },
    );

    // Builtin + custom profiles resolve; unknown names error.
    assert!(select_profile(&settings, "read-only").is_ok());
    assert!(select_profile(&settings, "unconfined").is_ok());
    assert!(select_profile(&settings, "custom").is_ok());
    assert!(select_profile(&settings, "nope").is_err());

    // Profile fields win over the base settings on overlay.
    let overlaid = overlay_profile(
        &settings,
        &crate::config::SandboxProfile {
            mode: Some("read-only".into()),
            ..Default::default()
        },
    );
    assert_eq!(overlaid.mode.as_deref(), Some("read-only"));
    assert_eq!(overlaid.network, Some(false)); // base preserved

    // disable beats everything.
    let disabled = resolve_with_overrides(
        &settings,
        dir.path(),
        &SandboxOverrides {
            disable: true,
            ..Default::default()
        },
        &[],
    )
    .unwrap();
    assert!(disabled.is_none());

    // read_only + strict_read overrides and extra roots land in the config.
    let extra = tempfile::tempdir().unwrap();
    let config = resolve_with_overrides(
        &settings,
        dir.path(),
        &SandboxOverrides {
            read_only: true,
            strict_read: true,
            ..Default::default()
        },
        &[extra.path().to_path_buf()],
    )
    .unwrap()
    .unwrap();
    assert_eq!(config.mode(), SandboxMode::ReadOnly);
    assert!(config.restrict_reads());
}
