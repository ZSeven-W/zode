use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::ConfigManager;
use crate::sessions::WorktreeMeta;
use crate::CoreError;

#[derive(Debug, Clone)]
pub struct WorktreeManager {
    root: PathBuf,
}

impl WorktreeManager {
    pub fn open_default() -> Result<Self, CoreError> {
        Ok(Self {
            root: ConfigManager::config_dir()?.join("worktrees"),
        })
    }

    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    /// Create a branch and linked worktree rooted at the source repository's
    /// current HEAD. No checkout in the user's existing worktree is changed.
    pub fn create(&self, source_cwd: &Path, session_id: &str) -> Result<WorktreeMeta, CoreError> {
        let repo = git_text(source_cwd, &["rev-parse", "--show-toplevel"])?;
        let repo = PathBuf::from(repo.trim());
        let base_commit = git_text(&repo, &["rev-parse", "HEAD"])?.trim().to_string();
        let short = session_id.chars().take(12).collect::<String>();
        let branch = format!("zode/session-{short}");
        let path = self.root.join(session_id);
        if path.exists() {
            return Err(CoreError::Other(format!(
                "worktree path already exists: {}",
                path.display()
            )));
        }
        std::fs::create_dir_all(&self.root)?;
        git_ok(
            &repo,
            &[
                "worktree",
                "add",
                "-b",
                &branch,
                path.to_string_lossy().as_ref(),
                &base_commit,
            ],
        )?;
        Ok(WorktreeMeta {
            path: path.display().to_string(),
            branch,
            base_commit,
        })
    }

    /// Apply the isolated worktree's tracked and untracked changes back to a
    /// target checkout using Git's 3-way apply. The isolated branch remains as
    /// a recovery point; callers may remove it explicitly after verification.
    pub fn apply_back(meta: &WorktreeMeta, target_cwd: &Path) -> Result<(), CoreError> {
        let source = Path::new(&meta.path);
        let tracked = git_bytes(
            source,
            &["diff", "--binary", "--full-index", &meta.base_commit, "--"],
        )?;
        let untracked = git_text(source, &["ls-files", "--others", "--exclude-standard"])?;
        let mut patch = tracked;
        for relative in untracked.lines().filter(|line| !line.trim().is_empty()) {
            let bytes = git_bytes(
                source,
                &["diff", "--binary", "--no-index", "/dev/null", relative],
            )?;
            patch.extend(bytes);
        }
        if patch.is_empty() {
            return Ok(());
        }
        let mut child = Command::new("git")
            .arg("-C")
            .arg(target_cwd)
            .args(["apply", "--3way", "--whitespace=nowarn", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .ok_or_else(|| CoreError::Other("git apply stdin unavailable".into()))?
            .write_all(&patch)?;
        let output = child.wait_with_output()?;
        if output.status.success() {
            Ok(())
        } else {
            Err(CoreError::Other(format!(
                "git apply-back failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            )))
        }
    }

    pub fn remove(meta: &WorktreeMeta) -> Result<(), CoreError> {
        let path = Path::new(&meta.path);
        let repo = git_text(path, &["rev-parse", "--show-toplevel"])?;
        git_ok(
            Path::new(repo.trim()),
            &["worktree", "remove", path.to_string_lossy().as_ref()],
        )
    }
}

fn git_ok(cwd: &Path, args: &[&str]) -> Result<(), CoreError> {
    let output = Command::new("git").arg("-C").arg(cwd).args(args).output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(CoreError::Other(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

fn git_text(cwd: &Path, args: &[&str]) -> Result<String, CoreError> {
    String::from_utf8(git_bytes(cwd, args)?)
        .map_err(|error| CoreError::Other(format!("git output was not UTF-8: {error}")))
}

fn git_bytes(cwd: &Path, args: &[&str]) -> Result<Vec<u8>, CoreError> {
    let output = Command::new("git").arg("-C").arg(cwd).args(args).output()?;
    if output.status.success() || output.status.code() == Some(1) && args.contains(&"--no-index") {
        Ok(output.stdout)
    } else {
        Err(CoreError::Other(format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(cwd: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(cwd)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn creates_isolated_worktree_and_applies_changes_back() {
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init"]);
        git(repo.path(), &["config", "user.email", "zode@example.test"]);
        git(repo.path(), &["config", "user.name", "Zode Test"]);
        std::fs::write(repo.path().join("tracked.txt"), b"before\n").unwrap();
        git(repo.path(), &["add", "tracked.txt"]);
        git(repo.path(), &["commit", "-m", "base"]);

        let worktrees = tempfile::tempdir().unwrap();
        let manager = WorktreeManager::at(worktrees.path().to_path_buf());
        let meta = manager.create(repo.path(), "session-123456789").unwrap();
        std::fs::write(Path::new(&meta.path).join("tracked.txt"), b"after\n").unwrap();
        std::fs::write(Path::new(&meta.path).join("new.txt"), b"new\n").unwrap();
        WorktreeManager::apply_back(&meta, repo.path()).unwrap();
        assert_eq!(
            std::fs::read(repo.path().join("tracked.txt")).unwrap(),
            b"after\n"
        );
        assert_eq!(
            std::fs::read(repo.path().join("new.txt")).unwrap(),
            b"new\n"
        );
    }
}
