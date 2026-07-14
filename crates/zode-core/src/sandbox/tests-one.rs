    use super::*;
    use serde_json::json;

    fn cfg(mode: SandboxMode, net: bool) -> SandboxConfig {
        SandboxConfig {
            os: SandboxOs::MacOs,
            cwd: PathBuf::from("/work/proj"),
            mode,
            allow_network: net,
            writable_roots: vec![],
            exclude_slash_tmp: false,
            exclude_tmpdir_env_var: false,
            restrict_reads: false,
            windows_tier: windows_policy::parse_windows_tier(None),
        }
    }

    #[test]
    fn strict_read_off_by_default_allows_reads() {
        let p = cfg(SandboxMode::WorkspaceWrite, false).macos_profile();
        assert!(
            !p.contains("(deny file-read*"),
            "reads must be unrestricted by default: {p}"
        );
    }

    #[test]
    fn strict_read_denies_credential_dir_reads() {
        // Only meaningful when there's a home dir to derive credential paths.
        if dirs::home_dir().is_none() {
            return;
        }
        let macos = cfg(SandboxMode::WorkspaceWrite, false)
            .with_restrict_reads(true)
            .macos_profile();
        assert!(
            macos.contains("(deny file-read*"),
            "strict-read must emit read denies: {macos}"
        );
        assert!(macos.contains(".ssh"), "must hide ~/.ssh: {macos}");

        let mut linux = cfg(SandboxMode::WorkspaceWrite, false).with_restrict_reads(true);
        linux.os = SandboxOs::Linux;
        let args = linux.linux_bwrap_prefix();
        // Collect the dirs masked with --tmpfs.
        let mut masked = Vec::new();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if a == "--tmpfs" {
                if let Some(p) = it.next() {
                    masked.push(p.clone());
                }
            }
        }
        // B3 invariant: only EXISTING dirs are masked (--tmpfs on an absent dir
        // would make bwrap fail and break every shell command).
        for m in &masked {
            assert!(
                std::path::Path::new(m).is_dir(),
                "must not --tmpfs a nonexistent dir: {m}"
            );
        }
        // If a known credential dir exists on this host, it must be masked.
        if let Some(home) = dirs::home_dir() {
            if home.join(".ssh").is_dir() {
                assert!(
                    masked.iter().any(|m| m.ends_with(".ssh")),
                    "existing ~/.ssh must be masked: {masked:?}"
                );
            }
        }
    }

    #[test]
    fn workspace_write_confines_writes_to_cwd_and_denies_network() {
        let p = cfg(SandboxMode::WorkspaceWrite, false).macos_profile();
        assert!(p.contains("(deny file-write*)"));
        assert!(p.contains("/work/proj"));
        assert!(p.contains("/tmp"));
        assert!(p.contains("(deny network*)"), "network denied by default");
        // standard devices stay writable so commands work
        assert!(p.contains("/dev/null"));
    }

    #[test]
    fn read_only_denies_all_writes_except_devices() {
        let p = cfg(SandboxMode::ReadOnly, false).macos_profile();
        assert!(p.contains("(deny file-write*)"));
        assert!(p.contains("/dev/null"), "devices still writable");
        // no workspace write-allow in read-only mode
        assert!(
            !p.contains("(allow file-write* (subpath \"/work/proj\"))"),
            "read-only must not allow writing the workspace: {p}"
        );
    }

    #[test]
    fn allow_network_omits_the_deny() {
        let p = cfg(SandboxMode::WorkspaceWrite, true).macos_profile();
        assert!(!p.contains("(deny network*)"), "network allowed");
    }

    #[test]
    fn extra_writable_roots_are_allowed() {
        let mut c = cfg(SandboxMode::WorkspaceWrite, false);
        c.writable_roots = vec![PathBuf::from("/data/cache")];
        let p = c.macos_profile();
        assert!(
            p.contains("/data/cache"),
            "extra writable root allowed: {p}"
        );
    }

    fn linux_cfg(mode: SandboxMode, net: bool) -> SandboxConfig {
        SandboxConfig {
            os: SandboxOs::Linux,
            cwd: PathBuf::from("/tmp"), // exists, so it binds
            mode,
            allow_network: net,
            writable_roots: vec![],
            exclude_slash_tmp: false,
            exclude_tmpdir_env_var: false,
            restrict_reads: false,
            windows_tier: windows_policy::parse_windows_tier(None),
        }
    }

    #[test]
    fn linux_unshares_net_by_default_and_runs_command() {
        let args = linux_cfg(SandboxMode::WorkspaceWrite, false).wrap("echo hi");
        assert_eq!(args[0], "bwrap");
        assert!(args.iter().any(|a| a == "--unshare-net"), "network dropped");
        assert!(args.iter().any(|a| a == "--bind"), "workspace bound rw");
        assert_eq!(args.last().unwrap(), "echo hi");
    }

    #[test]
    fn linux_read_only_has_no_rw_bind() {
        let args = linux_cfg(SandboxMode::ReadOnly, false).wrap("ls");
        assert!(!args.iter().any(|a| a == "--bind"), "read-only: no rw bind");
        assert!(args.iter().any(|a| a == "--ro-bind"));
    }

    #[test]
    fn linux_allow_network_keeps_net() {
        let args = linux_cfg(SandboxMode::WorkspaceWrite, true).wrap("curl x");
        assert!(!args.iter().any(|a| a == "--unshare-net"), "net kept");
    }

    #[test]
    fn mode_parse() {
        assert_eq!(SandboxMode::parse("read-only"), SandboxMode::ReadOnly);
        assert_eq!(SandboxMode::parse("readonly"), SandboxMode::ReadOnly);
        assert_eq!(
            SandboxMode::parse("workspace-write"),
            SandboxMode::WorkspaceWrite
        );
        assert_eq!(SandboxMode::parse("nonsense"), SandboxMode::WorkspaceWrite);
    }

    #[test]
    fn scheme_escape_handles_quotes_and_backslashes() {
        assert_eq!(scheme_escape(r#"a"b\c"#), r#"a\"b\\c"#);
        // A malicious cwd can't break out of the string literal.
        let mut c = cfg(SandboxMode::WorkspaceWrite, false);
        c.cwd = PathBuf::from(r#"/x") (allow file-write*) ;"#);
        let p = c.macos_profile();
        assert!(p.contains(r#"\""#), "quote must be escaped in profile");
    }

    /// Mock tool that records the `command` input it was called with.
    #[derive(Debug, Default)]
    struct RecordingTool {
        seen: std::sync::Mutex<Option<serde_json::Value>>,
    }
    #[async_trait]
    impl Tool for RecordingTool {
        fn name(&self) -> &str {
            "Bash"
        }
        fn description(&self) -> &str {
            "run a shell command"
        }
        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object", "properties": {"command": {"type": "string"}}})
        }
        fn safety_class(&self) -> SafetyClass {
            SafetyClass::Mutating
        }
        async fn call(
            &self,
            _ctx: &ToolUseContext,
            input: serde_json::Value,
        ) -> Result<serde_json::Value, AgentError> {
            *self.seen.lock().unwrap() = Some(input);
            Ok(serde_json::json!({}))
        }
    }

    fn allow_gate() -> Arc<dyn ApprovalGate> {
        Arc::new(crate::approval::BypassGate) // auto-approves, NOT interactive
    }

    /// Interactive mock gate: records every approve() label, returns a fixed
    /// answer — stands in for a human at the TUI/stdin prompt.
    #[derive(Debug)]
    struct PromptGate {
        answer: Approval,
        asked: std::sync::Mutex<Vec<String>>,
    }
    impl PromptGate {
        fn new(answer: Approval) -> Arc<Self> {
            Arc::new(Self {
                answer,
                asked: std::sync::Mutex::new(Vec::new()),
            })
        }
    }
    #[async_trait]
    impl ApprovalGate for PromptGate {
        fn interactive(&self) -> bool {
            true
        }
        async fn approve(&self, tool: &str, _input: &serde_json::Value) -> Approval {
            self.asked.lock().unwrap().push(tool.to_string());
            self.answer
        }
    }

    #[tokio::test]
    async fn escape_flag_runs_raw_and_is_stripped() {
        let rec = Arc::new(RecordingTool::default());
        let cfg = SandboxConfig::for_current_os(Path::new("/tmp")).unwrap();
        let gate = PromptGate::new(Approval::AllowOnce);
        let tool = SandboxedBashTool::new(rec.clone(), cfg, gate.clone());
        let ctx = ToolUseContext::new(std::env::temp_dir());

        // Without the escape flag → command is wrapped under the sandbox.
        tool.call(&ctx, serde_json::json!({"command": "echo hi"}))
            .await
            .unwrap();
        let wrapped = rec.seen.lock().unwrap().clone().unwrap();
        let cmd = wrapped["command"].as_str().unwrap();
        assert!(
            cmd.contains("sandbox-exec") || cmd.contains("bwrap"),
            "command should be sandbox-wrapped: {cmd}"
        );

        // With the escape flag (and the user approving the dedicated escape
        // prompt) → command runs raw and the flag is stripped.
        tool.call(
            &ctx,
            serde_json::json!({"command": "echo hi", ESCAPE_FLAG: true}),
        )
        .await
        .unwrap();
        let raw = rec.seen.lock().unwrap().clone().unwrap();
        assert_eq!(raw["command"].as_str().unwrap(), "echo hi", "ran raw");
        assert!(raw.get(ESCAPE_FLAG).is_none(), "escape flag stripped");
        assert_eq!(
            gate.asked.lock().unwrap().len(),
            1,
            "the escape must have its own approval prompt"
        );
    }

    #[tokio::test]
    async fn escape_is_not_honored_without_an_interactive_gate() {
        // yolo / bypass: no human to ask, so a model-requested escape must NOT
        // run raw — the sandbox config is the user's only contract there.
        let rec = Arc::new(RecordingTool::default());
        let cfg = SandboxConfig::for_current_os(Path::new("/tmp")).unwrap();
        let tool = SandboxedBashTool::new(rec.clone(), cfg, allow_gate());
        let ctx = ToolUseContext::new(std::env::temp_dir());
        tool.call(
            &ctx,
            json!({"command": "curl example.com", SANDBOX_PERMISSIONS_FLAG: "require_escalated"}),
        )
        .await
        .unwrap();
        let seen = rec.seen.lock().unwrap().clone().unwrap();
        let cmd = seen["command"].as_str().unwrap();
        assert!(
            cmd.contains("sandbox-exec") || cmd.contains("bwrap"),
            "escape under a bypass gate must stay sandboxed: {cmd}"
        );
    }

    #[tokio::test]
    async fn escape_denied_by_user_runs_sandboxed_without_second_offer() {
        // The user says no to the escape → run sandboxed; and even if that
        // fails on a sandbox denial, do NOT immediately re-ask to escalate.
        let inner = Arc::new(DenyThenRecord::default());
        let cfg = SandboxConfig::for_current_os(Path::new("/tmp")).unwrap();
        let gate = PromptGate::new(Approval::Deny);
        let tool = SandboxedBashTool::new(inner.clone(), cfg, gate.clone());
        let ctx = ToolUseContext::new(std::env::temp_dir());
        tool.call(
            &ctx,
            json!({"command": "curl example.com", SANDBOX_PERMISSIONS_FLAG: "require_escalated"}),
        )
        .await
        .unwrap();
        let calls = inner.calls.lock().unwrap();
        assert_eq!(
            calls.len(),
            1,
            "sandboxed run only, no raw re-run: {calls:?}"
        );
        assert!(
            calls[0].contains("sandbox-exec") || calls[0].contains("bwrap"),
            "the one run was sandboxed"
        );
        assert_eq!(
            gate.asked.lock().unwrap().len(),
            1,
            "asked once for the escape, not again for escalation"
        );
    }

    #[test]
    fn input_schema_advertises_sandbox_permissions_and_justification() {
        let rec = Arc::new(RecordingTool::default());
        let cfg = SandboxConfig::for_current_os(Path::new("/tmp")).unwrap();
        let tool = SandboxedBashTool::new(rec, cfg, allow_gate());
        let schema = tool.input_schema();
        let perms = &schema["properties"][SANDBOX_PERMISSIONS_FLAG];
        assert!(
            perms.is_object(),
            "sandbox_permissions advertised: {schema}"
        );
        let variants: Vec<&str> = perms["enum"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(variants.contains(&"use_default"));
        assert!(variants.contains(&"require_escalated"));
        assert!(schema["properties"].get(JUSTIFICATION_FLAG).is_some());
        // The legacy boolean is no longer advertised.
        assert!(schema["properties"].get(ESCAPE_FLAG).is_none());
    }

    #[tokio::test]
    async fn sandbox_permissions_require_escalated_runs_raw() {
        let rec = Arc::new(RecordingTool::default());
        let cfg = SandboxConfig::for_current_os(Path::new("/tmp")).unwrap();
        let gate = PromptGate::new(Approval::AllowOnce);
        let tool = SandboxedBashTool::new(rec.clone(), cfg, gate.clone());
        let ctx = ToolUseContext::new(std::env::temp_dir());
        tool.call(
            &ctx,
            json!({
                "command": "curl example.com",
                SANDBOX_PERMISSIONS_FLAG: "require_escalated",
                JUSTIFICATION_FLAG: "needs the network",
            }),
        )
        .await
        .unwrap();
        let seen = rec.seen.lock().unwrap().clone().unwrap();
        assert_eq!(
            seen["command"].as_str().unwrap(),
            "curl example.com",
            "ran raw"
        );
        assert_eq!(
            gate.asked.lock().unwrap().len(),
            1,
            "escape authorized via its own prompt"
        );
        assert!(
            seen.get(SANDBOX_PERMISSIONS_FLAG).is_none(),
            "control key stripped"
        );
        assert!(
            seen.get(JUSTIFICATION_FLAG).is_none(),
            "justification stripped"
        );
    }
