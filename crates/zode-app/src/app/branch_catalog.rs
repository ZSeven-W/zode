use std::{
    path::Path,
    process::{Command, Output},
    sync::Arc,
};

use tokio::sync::mpsc;
use winit::event_loop::EventLoopProxy;
use zode_app_model::{AppCommand, BranchCatalog};
use zode_app_runtime::workspace_uri_to_path;
use zode_node_protocol::WorkspaceUri;

use crate::window_state::AppWake;

use super::DesktopApp;

/// Loads repository facts away from the winit thread and reports them through
/// the same coalesced redraw path used by the other desktop effects.
pub(super) struct BranchCatalogEffect {
    result_sender: mpsc::UnboundedSender<AppCommand>,
    results: mpsc::UnboundedReceiver<AppCommand>,
    wake: Arc<dyn Fn() + Send + Sync>,
    in_flight: Vec<WorkspaceUri>,
}

impl BranchCatalogEffect {
    pub(super) fn new(proxy: EventLoopProxy<AppWake>) -> Self {
        Self::spawn_with_wake(move || {
            let _ = proxy.send_event(AppWake::Redraw);
        })
    }

    fn spawn_with_wake(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let (result_sender, results) = mpsc::unbounded_channel();
        Self {
            result_sender,
            results,
            wake: Arc::new(wake),
            in_flight: Vec::new(),
        }
    }

    /// Starts one workspace-scoped read. Duplicate requests for the same
    /// workspace share the in-flight result.
    pub(super) fn request(&mut self, workspace_uri: WorkspaceUri) {
        if self.in_flight.contains(&workspace_uri) {
            return;
        }
        self.in_flight.push(workspace_uri.clone());
        let sender = self.result_sender.clone();
        let wake = Arc::clone(&self.wake);
        tokio::spawn(async move {
            let command = match load_branch_catalog(workspace_uri.clone()).await {
                Ok(catalog) => AppCommand::BranchesLoaded(catalog),
                Err(message) => AppCommand::BranchesFailed {
                    workspace_uri,
                    message,
                },
            };
            if sender.send(command).is_ok() {
                wake();
            }
        });
    }

    /// Switches the checked-out local branch, then refreshes the catalog from
    /// Git so the UI only reports success after the repository confirms it.
    pub(super) fn switch(
        &mut self,
        workspace_uri: WorkspaceUri,
        expected_current: String,
        branch: String,
    ) {
        if self.in_flight.contains(&workspace_uri) {
            return;
        }
        self.in_flight.push(workspace_uri.clone());
        let sender = self.result_sender.clone();
        let wake = Arc::clone(&self.wake);
        tokio::spawn(async move {
            let command = match switch_branch(workspace_uri.clone(), expected_current, branch).await
            {
                Ok(catalog) => AppCommand::BranchesLoaded(catalog),
                Err(message) => AppCommand::BranchesFailed {
                    workspace_uri,
                    message,
                },
            };
            if sender.send(command).is_ok() {
                wake();
            }
        });
    }

    pub(super) fn drain(&mut self) -> Vec<AppCommand> {
        let mut drained = Vec::new();
        while let Ok(command) = self.results.try_recv() {
            let workspace = command_workspace(&command);
            if let Some(workspace) = workspace {
                self.in_flight.retain(|candidate| candidate != workspace);
            }
            drained.push(command);
        }
        drained
    }
}

impl DesktopApp {
    pub(super) fn request_branch_catalog(&mut self, workspace_uri: WorkspaceUri) {
        self.branch_catalog.request(workspace_uri);
    }

    pub(super) fn request_branch_switch(
        &mut self,
        workspace_uri: WorkspaceUri,
        expected_current: String,
        branch: String,
    ) {
        self.branch_catalog
            .switch(workspace_uri, expected_current, branch);
    }

    pub(super) fn drain_branch_catalog_results(&mut self) -> usize {
        let commands = self.branch_catalog.drain();
        let count = commands.len();
        for command in commands {
            self.enqueue_command(command);
        }
        count
    }
}

fn command_workspace(command: &AppCommand) -> Option<&WorkspaceUri> {
    match command {
        AppCommand::BranchesLoaded(catalog) => Some(&catalog.workspace_uri),
        AppCommand::BranchesFailed { workspace_uri, .. } => Some(workspace_uri),
        _ => None,
    }
}

async fn load_branch_catalog(workspace_uri: WorkspaceUri) -> Result<BranchCatalog, String> {
    let worker_workspace = workspace_uri.clone();
    tokio::task::spawn_blocking(move || load_branch_catalog_blocking(worker_workspace))
        .await
        .map_err(|error| format!("branch catalog worker failed: {error}"))?
}

async fn switch_branch(
    workspace_uri: WorkspaceUri,
    expected_current: String,
    branch: String,
) -> Result<BranchCatalog, String> {
    let worker_workspace = workspace_uri.clone();
    tokio::task::spawn_blocking(move || {
        let path = workspace_uri_to_path(&worker_workspace)
            .map_err(|error| format!("workspace is not a local directory: {error}"))?
            .canonicalize()
            .map_err(|error| format!("workspace directory is unavailable: {error}"))?;
        ensure_repository_root(&path)?;
        validate_branch_name(&path, &branch)?;
        let before = load_branch_catalog_blocking(worker_workspace.clone())?;
        if before.current != expected_current {
            return Err("the checked-out branch changed; reopen the branch menu".into());
        }
        if before.dirty_files > 0 {
            return Err("commit or stash local changes before switching branches".into());
        }
        let branch_ref = format!("refs/heads/{branch}");
        git_output(
            &path,
            &["show-ref", "--verify", "--quiet", branch_ref.as_str()],
        )
        .map_err(|_| "the selected local branch is no longer available".to_owned())?;
        git_output(&path, &["switch", "--quiet", "--no-guess", &branch])?;
        let confirmed = current_branch(&path)?;
        if confirmed != branch {
            return Err("Git did not confirm the requested branch checkout".into());
        }
        match load_branch_catalog_blocking(worker_workspace) {
            Ok(catalog) => Ok(catalog),
            Err(_) => Ok(BranchCatalog {
                current: confirmed,
                ..before
            }),
        }
    })
    .await
    .map_err(|error| format!("branch switch worker failed: {error}"))?
}

fn validate_branch_name(path: &Path, branch: &str) -> Result<(), String> {
    if branch.is_empty() || branch.starts_with('-') {
        return Err("the selected branch name is invalid".into());
    }
    git_output(path, &["check-ref-format", "--branch", branch]).map(|_| ())
}

fn ensure_repository_root(path: &Path) -> Result<(), String> {
    let root = utf8_output(git_output(path, &["rev-parse", "--show-toplevel"])?)?;
    let root = Path::new(root.trim())
        .canonicalize()
        .map_err(|error| format!("repository root is unavailable: {error}"))?;
    if root != path {
        return Err(
            "branch checkout requires the project workspace to be the repository root".into(),
        );
    }
    Ok(())
}

fn load_branch_catalog_blocking(workspace_uri: WorkspaceUri) -> Result<BranchCatalog, String> {
    let path = workspace_uri_to_path(&workspace_uri)
        .map_err(|error| format!("workspace is not a local directory: {error}"))?
        .canonicalize()
        .map_err(|error| format!("workspace directory is unavailable: {error}"))?;

    git_output(&path, &["rev-parse", "--is-inside-work-tree"])?;
    let current = current_branch(&path)?;
    let mut branches = utf8_output(git_output(
        &path,
        &["for-each-ref", "--format=%(refname:short)", "refs/heads"],
    )?)?
    .lines()
    .map(str::trim)
    .filter(|branch| !branch.is_empty())
    .map(str::to_owned)
    .collect::<Vec<_>>();
    branches.sort();
    branches.dedup();

    let status = git_output(
        &path,
        &["status", "--porcelain=v2", "-z", "--untracked-files=normal"],
    )?;
    let dirty_files = status
        .split(|byte| *byte == 0)
        .filter(|record| is_dirty_status_record(record))
        .count();

    Ok(BranchCatalog {
        workspace_uri,
        current,
        branches,
        dirty_files,
    })
}

fn current_branch(path: &Path) -> Result<String, String> {
    let symbolic = git_process(path, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
    if symbolic.status.success() {
        return utf8_output(symbolic.stdout).map(|branch| branch.trim().to_owned());
    }
    let revision = utf8_output(git_output(path, &["rev-parse", "--short", "HEAD"])?)?;
    Ok(format!("HEAD ({})", revision.trim()))
}

fn git_output(path: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = git_process(path, args)?;
    if output.status.success() {
        return Ok(output.stdout);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.trim();
    Err(if detail.is_empty() {
        format!("git {} failed with {}", args.join(" "), output.status)
    } else {
        format!("git {} failed: {detail}", args.join(" "))
    })
}

fn git_process(path: &Path, args: &[&str]) -> Result<Output, String> {
    Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .map_err(|error| format!("could not run git: {error}"))
}

fn utf8_output(bytes: Vec<u8>) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|_| "git returned non-UTF-8 branch data".to_owned())
}

fn is_dirty_status_record(record: &[u8]) -> bool {
    record.starts_with(b"1 ")
        || record.starts_with(b"2 ")
        || record.starts_with(b"u ")
        || record.starts_with(b"? ")
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf, sync::Arc};

    use tokio::sync::Notify;
    use zode_app_model::AppCommand;
    use zode_app_runtime::path_to_workspace_uri;

    use super::{load_branch_catalog, switch_branch, BranchCatalogEffect};

    struct TestRepository(PathBuf);

    impl TestRepository {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "zode-branch-catalog-{}",
                uuid::Uuid::new_v4().simple()
            ));
            fs::create_dir_all(&path).unwrap();
            run_git(&path, &["init", "--quiet", "--initial-branch=main"]);
            run_git(&path, &["config", "user.email", "zode@example.invalid"]);
            run_git(&path, &["config", "user.name", "Zode Test"]);
            fs::write(path.join("README.md"), "zode\n").unwrap();
            run_git(&path, &["add", "README.md"]);
            run_git(&path, &["commit", "--quiet", "-m", "initial"]);
            Self(path)
        }

        fn workspace_uri(&self) -> zode_node_protocol::WorkspaceUri {
            path_to_workspace_uri(&self.0).unwrap()
        }
    }

    impl Drop for TestRepository {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn run_git(path: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {} failed", args.join(" "));
    }

    use std::{path::Path, process::Command};

    #[tokio::test]
    async fn loads_current_local_branches_and_dirty_file_count() {
        let repository = TestRepository::new();
        run_git(&repository.0, &["branch", "feature/context-menu"]);
        fs::write(repository.0.join("README.md"), "changed\n").unwrap();
        fs::write(repository.0.join("untracked.txt"), "new\n").unwrap();

        let catalog = load_branch_catalog(repository.workspace_uri())
            .await
            .unwrap();

        assert_eq!(catalog.current, "main");
        assert_eq!(
            catalog.branches,
            vec!["feature/context-menu".to_owned(), "main".to_owned()]
        );
        assert_eq!(catalog.dirty_files, 2);
    }

    #[tokio::test]
    async fn effect_wakes_and_drains_a_workspace_scoped_command() {
        let repository = TestRepository::new();
        let wake = Arc::new(Notify::new());
        let worker_wake = Arc::clone(&wake);
        let mut effect = BranchCatalogEffect::spawn_with_wake(move || worker_wake.notify_one());
        let workspace = repository.workspace_uri();

        effect.request(workspace.clone());
        tokio::time::timeout(std::time::Duration::from_secs(5), wake.notified())
            .await
            .expect("branch worker should wake the app");

        let commands = effect.drain();
        assert!(matches!(
            commands.as_slice(),
            [AppCommand::BranchesLoaded(catalog)] if catalog.workspace_uri == workspace
        ));
    }

    #[tokio::test]
    async fn switches_a_valid_local_branch_before_reporting_it_selected() {
        let repository = TestRepository::new();
        run_git(&repository.0, &["branch", "feature/context-menu"]);

        let catalog = switch_branch(
            repository.workspace_uri(),
            "main".to_owned(),
            "feature/context-menu".to_owned(),
        )
        .await
        .unwrap();

        assert_eq!(catalog.current, "feature/context-menu");
    }

    #[tokio::test]
    async fn refuses_a_stale_or_dirty_local_checkout() {
        let repository = TestRepository::new();
        run_git(&repository.0, &["branch", "feature/context-menu"]);

        let stale = switch_branch(
            repository.workspace_uri(),
            "stale".to_owned(),
            "feature/context-menu".to_owned(),
        )
        .await
        .unwrap_err();
        assert!(stale.contains("checked-out branch changed"));

        fs::write(repository.0.join("README.md"), "dirty\n").unwrap();
        let dirty = switch_branch(
            repository.workspace_uri(),
            "main".to_owned(),
            "feature/context-menu".to_owned(),
        )
        .await
        .unwrap_err();
        assert!(dirty.contains("commit or stash"));
    }
}
