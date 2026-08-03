//! Install / uninstall a plugin from git. Shallow-clones `owner/repo`
//! (assumed `github.com`) or a full git URL (`https://…`, `git@host:…`, or a
//! bare local path — the last only realistic for tests/dev, git itself
//! treats a filesystem path as a valid clone source) into
//! `<config_dir>/plugins/<sanitized-id>/`, scans its tree for capabilities,
//! and writes `manifest.json`. Shells out to the system `git` binary (no
//! libgit2 dependency), the same approach as the environment panel's git
//! data source (`crate::git_stat`).
//!
//! Updating an existing install (fetch + re-sync + trust re-review) lives in
//! [`super::update`], which reuses this module's git-subprocess plumbing
//! (`run_git`, `classify_git_failure`, `GIT_TIMEOUT`).
//!
//! Not implemented in this wave (out of the approved v1 scope, see the
//! design doc): sparse-checkout paths.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::io::AsyncReadExt;

use super::manifest::PluginManifest;
use super::scan::scan_capabilities;
use super::trust::TrustStore;
use super::update;
use super::PluginMarketError;

/// Shallow-clone timeout — a hung/unreachable remote must never hang the
/// install indefinitely.
pub(super) const GIT_TIMEOUT: Duration = Duration::from_secs(60);

/// Directory name a plugin installs under, plus its metadata after install.
#[derive(Debug, Clone)]
pub struct InstalledPlugin {
    /// The sanitized directory name — also used as the trust.json plugin id.
    pub id: String,
    pub dir: PathBuf,
    pub manifest: PluginManifest,
}

/// `<config_dir>/plugins` — every install lives directly under this root.
pub fn plugins_root(config_dir: &Path) -> PathBuf {
    config_dir.join("plugins")
}

/// Install `spec` (`owner/repo`, a full git URL, or `git@host:owner/repo`) at
/// an optional pinned `reference` (branch/tag/sha).
pub async fn install(
    spec: &str,
    reference: Option<&str>,
    config_dir: &Path,
) -> Result<InstalledPlugin, PluginMarketError> {
    let (clone_url, id) = resolve_spec(spec)?;
    let root = plugins_root(config_dir);
    std::fs::create_dir_all(&root)?;
    let root_canon = std::fs::canonicalize(&root)?;
    let dest = root.join(&id);
    if dest.exists() {
        return Err(PluginMarketError::AlreadyInstalled(id));
    }

    let dest_str = dest.to_string_lossy().to_string();
    let mut args = vec!["clone", "--depth", "1"];
    if let Some(r) = reference {
        args.push("--branch");
        args.push(r);
    }
    args.push(&clone_url);
    args.push(&dest_str);

    let output = match run_git(&args, None, GIT_TIMEOUT).await {
        Ok(out) => out,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dest);
            return Err(e);
        }
    };
    if !output.status.success() {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(classify_git_failure(&output.stderr));
    }

    // Re-check under the canonical root: defends against a symlink swapped
    // in during the clone (belt-and-braces alongside the sanitized `id`,
    // which already rules out `..`/absolute-path components).
    let dest_canon = std::fs::canonicalize(&dest)?;
    if !dest_canon.starts_with(&root_canon) {
        let _ = std::fs::remove_dir_all(&dest);
        return Err(PluginMarketError::UnsafePath(
            "clone target escaped the plugins root".into(),
        ));
    }

    let capabilities = scan_capabilities(&dest_canon);
    // Best-effort: a manifest without a commit still installs and still
    // updates later (the update path re-resolves `HEAD` itself).
    let commit = update::rev_parse(&dest_canon, "HEAD").await.ok();
    let manifest = PluginManifest {
        repo: spec.to_string(),
        reference: reference.unwrap_or("HEAD").to_string(),
        installed_at: now_millis(),
        commit,
        updated_at: None,
        capabilities,
    };
    manifest.save(&dest_canon)?;

    Ok(InstalledPlugin {
        id,
        dir: dest_canon,
        manifest,
    })
}

/// Remove an installed plugin's directory and its trust.json record. Refuses
/// to touch anything outside `<config_dir>/plugins` even if `id` were crafted
/// to try to escape it (canonicalized-prefix check, same guard as install).
pub fn uninstall(id: &str, config_dir: &Path) -> Result<(), PluginMarketError> {
    let root = plugins_root(config_dir);
    let root_canon = std::fs::canonicalize(&root)
        .map_err(|_| PluginMarketError::NotInstalled(id.to_string()))?;
    let target = root.join(id);
    if !target.exists() {
        return Err(PluginMarketError::NotInstalled(id.to_string()));
    }
    let target_canon = std::fs::canonicalize(&target)?;
    if !target_canon.starts_with(&root_canon) {
        return Err(PluginMarketError::UnsafePath(
            "refusing to delete a path outside the plugins root".into(),
        ));
    }
    std::fs::remove_dir_all(&target_canon)?;

    if let Ok(mut trust) = TrustStore::load(config_dir) {
        trust.remove_plugin(id);
        let _ = trust.save();
    }
    Ok(())
}

pub(super) fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

/// Run one `git` subprocess bounded by `timeout`.
pub(super) async fn run_git(
    args: &[&str],
    cwd: Option<&Path>,
    timeout: Duration,
) -> Result<std::process::Output, PluginMarketError> {
    run_timed("git", args, cwd, timeout)
        .await
        .map_err(|e| match e {
            RunError::NotFound => PluginMarketError::GitMissing,
            RunError::Io(e) => PluginMarketError::Io(e),
            RunError::Timeout => PluginMarketError::Timeout,
        })
}

enum RunError {
    NotFound,
    Io(std::io::Error),
    Timeout,
}

/// Spawn `program` bounded by `timeout`; on timeout, kill the child rather
/// than leaving it running orphaned. Generic over `program` so the timeout
/// mechanism itself can be unit-tested without touching git or the network
/// (see `run_timed_kills_and_errors_on_timeout` below).
async fn run_timed(
    program: &str,
    args: &[&str],
    cwd: Option<&Path>,
    timeout: Duration,
) -> Result<std::process::Output, RunError> {
    let mut cmd = tokio::process::Command::new(program);
    if let Some(cwd) = cwd {
        cmd.current_dir(cwd);
    }
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().map_err(|e| {
        if e.kind() == ErrorKind::NotFound {
            RunError::NotFound
        } else {
            RunError::Io(e)
        }
    })?;
    let mut stdout_pipe = child.stdout.take();
    let mut stderr_pipe = child.stderr.take();

    let status = match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) => status,
        Ok(Err(e)) => return Err(RunError::Io(e)),
        Err(_) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(RunError::Timeout);
        }
    };

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    if let Some(mut p) = stdout_pipe.take() {
        let _ = p.read_to_end(&mut stdout).await;
    }
    if let Some(mut p) = stderr_pipe.take() {
        let _ = p.read_to_end(&mut stderr).await;
    }
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

/// Map git's stderr text to a structured error. Best-effort pattern match on
/// git's (not machine-stable, but long-unchanged) English error strings.
pub(super) fn classify_git_failure(stderr: &[u8]) -> PluginMarketError {
    let text = String::from_utf8_lossy(stderr).to_lowercase();
    if text.contains("repository not found")
        || text.contains("not found")
        || text.contains("does not exist")
    {
        PluginMarketError::NotFound(text.trim().to_string())
    } else if text.contains("authentication")
        || text.contains("could not read username")
        || text.contains("permission denied")
    {
        PluginMarketError::AuthRequired(text.trim().to_string())
    } else {
        PluginMarketError::GitFailed(text.trim().to_string())
    }
}

/// Resolve a user-supplied spec to `(clone_url, sanitized_id)`.
///
/// Three accepted forms:
/// - `owner/repo` (exactly one `/`, both sides safe) -> assumed `github.com`,
///   id `owner__repo`.
/// - a full URL (`scheme://…`) or scp-like (`git@host:owner/repo.git`) -> the
///   URL is used verbatim as the clone source; the id is derived from its
///   path (last one or two safe components, `.git` suffix stripped).
/// - a bare absolute local filesystem path -> same derivation as above (this
///   form only exists for hermetic tests against a local fixture repo; git
///   itself accepts a plain path as a clone source).
///
/// Every path component considered for the id is validated against a strict
/// allowlist (`is_safe_component`) and any `.`/`..` component is rejected
/// outright rather than sanitized-and-continued — this is the sole defense
/// against path traversal via a crafted `spec`, since the id becomes a
/// literal path segment joined under the plugins root.
fn resolve_spec(spec: &str) -> Result<(String, String), PluginMarketError> {
    let trimmed = spec.trim();
    if trimmed.is_empty() {
        return Err(PluginMarketError::InvalidSpec("empty plugin spec".into()));
    }

    if let Some((owner, repo)) = trimmed.split_once('/') {
        if !repo.contains('/') && is_safe_component(owner) && is_safe_component(repo) {
            let clone_url = format!("https://github.com/{owner}/{repo}.git");
            let id = format!("{owner}__{repo}");
            return Ok((clone_url, id));
        }
    }

    if trimmed.contains("://") || trimmed.starts_with("git@") {
        let id = derive_dir_name(&path_part_of(trimmed))?;
        return Ok((trimmed.to_string(), id));
    }

    if trimmed.starts_with('/') {
        let id = derive_dir_name(trimmed)?;
        return Ok((trimmed.to_string(), id));
    }

    Err(PluginMarketError::InvalidSpec(format!(
        "cannot parse plugin spec: {trimmed}"
    )))
}

/// The `owner/repo[.git]`-shaped tail of a URL or scp-like spec: everything
/// after the host. `https://host/a/b` -> `a/b`; `git@host:a/b.git` -> `a/b.git`.
fn path_part_of(spec: &str) -> String {
    if let Some((_, after_scheme)) = spec.split_once("://") {
        match after_scheme.split_once('/') {
            Some((_host, rest)) => rest.to_string(),
            None => String::new(),
        }
    } else if let Some(rest) = spec.strip_prefix("git@") {
        match rest.split_once(':') {
            Some((_host, path)) => path.to_string(),
            None => String::new(),
        }
    } else {
        spec.to_string()
    }
}

/// A single path component is safe iff it's non-empty, isn't `.`/`..`, and
/// contains only `[A-Za-z0-9._-]`. Rejects anything else (including empty
/// components from `//`) rather than trying to "clean" it.
fn is_safe_component(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// Turn a `/`-separated path into a directory name: strip a trailing `.git`,
/// validate every non-empty component, and join the last one or two
/// components with `__`. Any unsafe component anywhere in the path aborts
/// the whole resolution (`UnsafePath`) rather than silently dropping it.
fn derive_dir_name(path: &str) -> Result<String, PluginMarketError> {
    let stripped = path.strip_suffix(".git").unwrap_or(path);
    let components: Vec<&str> = stripped.split('/').filter(|s| !s.is_empty()).collect();
    if components.is_empty() {
        return Err(PluginMarketError::InvalidSpec(
            "plugin spec has no path component to name the install from".into(),
        ));
    }
    for c in &components {
        if !is_safe_component(c) {
            return Err(PluginMarketError::UnsafePath(format!(
                "unsafe path component `{c}` in plugin spec"
            )));
        }
    }
    let tail = if components.len() >= 2 {
        format!(
            "{}__{}",
            components[components.len() - 2],
            components[components.len() - 1]
        )
    } else {
        components[0].to_string()
    };
    Ok(tail)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init_fixture_repo(dir: &Path) {
        std::fs::create_dir_all(dir.join("skills/demo")).unwrap();
        std::fs::write(
            dir.join("skills/demo/SKILL.md"),
            "---\ndescription: demo skill\n---\nDo the demo.",
        )
        .unwrap();
        std::fs::write(dir.join("README.md"), "fixture plugin\n").unwrap();
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .current_dir(dir)
                .args(args)
                .status()
                .expect("git available for fixture setup");
            assert!(status.success(), "git {args:?} failed");
        };
        run(&["init", "-q"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        run(&["add", "-A"]);
        run(&["commit", "-q", "-m", "init"]);
    }

    #[tokio::test]
    async fn install_happy_path_clones_and_scans() {
        let fixture = tempfile::tempdir().unwrap();
        init_fixture_repo(fixture.path());
        let config_dir = tempfile::tempdir().unwrap();

        let spec = fixture.path().to_string_lossy().to_string();
        let installed = install(&spec, None, config_dir.path()).await.unwrap();

        assert!(installed.dir.join("README.md").exists());
        assert!(PluginManifest::load(&installed.dir).is_ok());
        assert!(installed.manifest.capabilities.iter().any(|c| matches!(
            c,
            crate::plugin_market::manifest::Capability::Skill { name, .. } if name == "demo"
        )));
    }

    #[tokio::test]
    async fn install_pins_the_requested_ref() {
        let fixture = tempfile::tempdir().unwrap();
        init_fixture_repo(fixture.path());
        let run = |args: &[&str]| {
            let status = std::process::Command::new("git")
                .current_dir(fixture.path())
                .args(args)
                .status()
                .unwrap();
            assert!(status.success());
        };
        run(&["tag", "v1"]);
        std::fs::write(fixture.path().join("README.md"), "changed\n").unwrap();
        run(&["add", "-A"]);
        run(&["commit", "-q", "-m", "second"]);

        let config_dir = tempfile::tempdir().unwrap();
        let spec = fixture.path().to_string_lossy().to_string();
        let installed = install(&spec, Some("v1"), config_dir.path()).await.unwrap();
        let content = std::fs::read_to_string(installed.dir.join("README.md")).unwrap();
        assert_eq!(content, "fixture plugin\n");
        assert_eq!(installed.manifest.reference, "v1");
        assert!(installed.manifest.commit.is_some());
    }

    #[tokio::test]
    async fn install_rejects_path_traversal_repo_spec() {
        let config_dir = tempfile::tempdir().unwrap();
        let err = install("../../evil", None, config_dir.path())
            .await
            .unwrap_err();
        assert!(matches!(err, PluginMarketError::InvalidSpec(_)));
    }

    #[tokio::test]
    async fn install_rejects_traversal_inside_absolute_spec() {
        let config_dir = tempfile::tempdir().unwrap();
        let err = install("/tmp/../../etc/passwd", None, config_dir.path())
            .await
            .unwrap_err();
        assert!(matches!(err, PluginMarketError::UnsafePath(_)));
    }

    #[tokio::test]
    async fn install_rejects_owner_repo_with_slash_smuggled_into_repo_side() {
        let config_dir = tempfile::tempdir().unwrap();
        let err = install("owner/../evil", None, config_dir.path())
            .await
            .unwrap_err();
        assert!(matches!(err, PluginMarketError::InvalidSpec(_)));
    }

    #[tokio::test]
    async fn install_rejects_dotdot_inside_a_full_url_path() {
        let config_dir = tempfile::tempdir().unwrap();
        let err = install(
            "https://github.com/acme/../../etc/passwd",
            None,
            config_dir.path(),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, PluginMarketError::UnsafePath(_)));
    }

    #[tokio::test]
    async fn uninstall_removes_directory_and_refuses_outside_root() {
        let fixture = tempfile::tempdir().unwrap();
        init_fixture_repo(fixture.path());
        let config_dir = tempfile::tempdir().unwrap();
        let spec = fixture.path().to_string_lossy().to_string();
        let installed = install(&spec, None, config_dir.path()).await.unwrap();
        assert!(installed.dir.exists());

        uninstall(&installed.id, config_dir.path()).unwrap();
        assert!(!installed.dir.exists());

        // Uninstalling again is a clean NotInstalled, not a panic/IO error.
        let err = uninstall(&installed.id, config_dir.path()).unwrap_err();
        assert!(matches!(err, PluginMarketError::NotInstalled(_)));
    }

    #[tokio::test]
    async fn uninstall_refuses_an_id_that_escapes_the_root() {
        let config_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(plugins_root(config_dir.path())).unwrap();
        // `..` resolves (via normal path-component resolution) to the config
        // dir itself, which exists but sits outside the canonicalized plugins
        // root — this locks in that uninstall never deletes outside the root
        // even when handed a strange id, independent of `resolve_spec`
        // already rejecting `..` at install time.
        let err = uninstall("..", config_dir.path()).unwrap_err();
        assert!(matches!(err, PluginMarketError::UnsafePath(_)));
    }

    #[tokio::test]
    async fn double_install_is_rejected() {
        let fixture = tempfile::tempdir().unwrap();
        init_fixture_repo(fixture.path());
        let config_dir = tempfile::tempdir().unwrap();
        let spec = fixture.path().to_string_lossy().to_string();
        install(&spec, None, config_dir.path()).await.unwrap();
        let err = install(&spec, None, config_dir.path()).await.unwrap_err();
        assert!(matches!(err, PluginMarketError::AlreadyInstalled(_)));
    }

    #[test]
    fn resolve_spec_accepts_owner_repo_shorthand() {
        let (url, id) = resolve_spec("acme/tools").unwrap();
        assert_eq!(url, "https://github.com/acme/tools.git");
        assert_eq!(id, "acme__tools");
    }

    #[test]
    fn resolve_spec_accepts_full_https_url() {
        let (url, id) = resolve_spec("https://github.com/acme/tools.git").unwrap();
        assert_eq!(url, "https://github.com/acme/tools.git");
        assert_eq!(id, "acme__tools");
    }

    #[test]
    fn resolve_spec_accepts_scp_like_git_url() {
        let (_, id) = resolve_spec("git@github.com:acme/tools.git").unwrap();
        assert_eq!(id, "acme__tools");
    }

    #[test]
    fn resolve_spec_rejects_empty() {
        assert!(matches!(
            resolve_spec("").unwrap_err(),
            PluginMarketError::InvalidSpec(_)
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn run_timed_kills_and_errors_on_timeout() {
        let err = run_timed("sleep", &["5"], None, Duration::from_millis(50))
            .await
            .expect_err("expected a timeout error");
        assert!(matches!(err, RunError::Timeout));
    }
}
