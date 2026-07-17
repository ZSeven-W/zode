//! Working-tree diff for the `/diff` command. Shells out to the system `git`
//! (matching `tools::git`) and returns a combined unstaged + staged diff as
//! plain text, ready to push into the chat view.

use std::path::Path;

use crate::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreDiffFileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreDiffFile {
    pub path: String,
    pub status: CoreDiffFileStatus,
    pub additions: u32,
    pub deletions: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CoreDiffSnapshot {
    pub files: Vec<CoreDiffFile>,
    pub unified: String,
}

/// Run `git diff <args>` in `cwd`, returning stdout (empty string on any
/// failure — callers treat "no diff" and "not a repo" the same).
async fn git_diff(cwd: &Path, extra: &[&str]) -> String {
    let mut args = vec!["--no-pager", "diff", "--no-color"];
    args.extend_from_slice(extra);
    match tokio::process::Command::new("git")
        .args(&args)
        .current_dir(cwd)
        .output()
        .await
    {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).into_owned(),
        _ => String::new(),
    }
}

/// List untracked (and not-ignored) files, one per line. Empty if none / not a
/// repo. `git diff` never shows these, so `/diff` would otherwise hide new
/// files entirely.
async fn untracked_files(cwd: &Path) -> String {
    match tokio::process::Command::new("git")
        .args(["ls-files", "--others", "--exclude-standard"])
        .current_dir(cwd)
        .output()
        .await
    {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).into_owned(),
        _ => String::new(),
    }
}

async fn git_status(cwd: &Path) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .args(["status", "--porcelain=v1"])
        .current_dir(cwd)
        .output()
        .await
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// One point-in-time working-tree projection owned by core. A non-repository
/// directory is a valid empty snapshot so desktop review never crashes.
pub async fn diff_snapshot(cwd: &Path) -> Result<CoreDiffSnapshot, CoreError> {
    let Some(status) = git_status(cwd).await else {
        return Ok(CoreDiffSnapshot::default());
    };
    let numstat = git_diff(cwd, &["--numstat", "HEAD"]).await;
    let counts = parse_numstat(&numstat);
    let files = parse_status(cwd, &status, &counts);

    let mut unified = git_diff(cwd, &["HEAD"]).await;
    if unified.is_empty() {
        unified.push_str(&git_diff(cwd, &["--staged"]).await);
        unified.push_str(&git_diff(cwd, &[]).await);
    }
    for file in files
        .iter()
        .filter(|file| file.status == CoreDiffFileStatus::Untracked)
    {
        unified.push_str(&untracked_patch(cwd, &file.path));
    }

    Ok(CoreDiffSnapshot { files, unified })
}

fn parse_status(
    cwd: &Path,
    status: &str,
    counts: &std::collections::HashMap<String, (u32, u32)>,
) -> Vec<CoreDiffFile> {
    status
        .lines()
        .filter_map(|line| {
            if line.len() < 4 {
                return None;
            }
            let code = &line[..2];
            let raw_path = &line[3..];
            let path = raw_path
                .rsplit(" -> ")
                .next()
                .unwrap_or(raw_path)
                .trim_matches('"')
                .to_owned();
            let file_status = if code == "??" {
                CoreDiffFileStatus::Untracked
            } else if code.contains('R') {
                CoreDiffFileStatus::Renamed
            } else if code.contains('D') {
                CoreDiffFileStatus::Deleted
            } else if code.contains('A') {
                CoreDiffFileStatus::Added
            } else {
                CoreDiffFileStatus::Modified
            };
            let (additions, deletions) = counts.get(&path).copied().unwrap_or_else(|| {
                if file_status == CoreDiffFileStatus::Untracked {
                    let additions = std::fs::read_to_string(cwd.join(&path))
                        .map(|content| content.lines().count() as u32)
                        .unwrap_or(0);
                    (additions, 0)
                } else {
                    (0, 0)
                }
            });
            Some(CoreDiffFile {
                path,
                status: file_status,
                additions,
                deletions,
            })
        })
        .collect()
}

fn parse_numstat(output: &str) -> std::collections::HashMap<String, (u32, u32)> {
    output
        .lines()
        .filter_map(|line| {
            let mut columns = line.splitn(3, '\t');
            let additions = columns.next()?.parse().ok()?;
            let deletions = columns.next()?.parse().ok()?;
            let path = columns.next()?.trim_matches('"').to_owned();
            Some((path, (additions, deletions)))
        })
        .collect()
}

fn untracked_patch(cwd: &Path, path: &str) -> String {
    let Ok(bytes) = std::fs::read(cwd.join(path)) else {
        return String::new();
    };
    let header = format!(
        "diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n"
    );
    let Ok(content) = String::from_utf8(bytes) else {
        return format!("{header}Binary files /dev/null and b/{path} differ\n");
    };
    let count = content.lines().count();
    let mut patch = format!("{header}@@ -0,0 +1,{count} @@\n");
    for line in content.lines() {
        patch.push('+');
        patch.push_str(line);
        patch.push('\n');
    }
    patch
}

/// Combined working-tree diff: staged changes first (what a commit would
/// record), then unstaged, then a list of untracked files. Returns a friendly
/// message when the tree is clean or the directory is not a git repository.
pub async fn working_tree_diff(cwd: &Path) -> String {
    let staged = git_diff(cwd, &["--staged"]).await;
    let unstaged = git_diff(cwd, &[]).await;
    let untracked = untracked_files(cwd).await;

    let mut out = String::new();
    if !staged.trim().is_empty() {
        out.push_str("# Staged changes\n");
        out.push_str(&staged);
        if !staged.ends_with('\n') {
            out.push('\n');
        }
    }
    if !unstaged.trim().is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("# Unstaged changes\n");
        out.push_str(&unstaged);
    }
    if !untracked.trim().is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("# Untracked files\n");
        for f in untracked.lines() {
            out.push_str("  ");
            out.push_str(f);
            out.push('\n');
        }
    }
    if out.trim().is_empty() {
        "(working tree clean — no changes to diff)".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(cwd: &Path, args: &[&str]) {
        let status = std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn fixture_git_repo() -> tempfile::TempDir {
        let repo = tempfile::tempdir().unwrap();
        git(repo.path(), &["init", "-q"]);
        git(repo.path(), &["config", "user.email", "test@example.com"]);
        git(repo.path(), &["config", "user.name", "Test"]);
        std::fs::write(repo.path().join("a.txt"), "one\n").unwrap();
        git(repo.path(), &["add", "a.txt"]);
        git(repo.path(), &["commit", "-qm", "initial"]);
        repo
    }

    #[tokio::test]
    async fn non_repo_dir_reports_clean() {
        let dir = tempfile::tempdir().unwrap();
        let out = working_tree_diff(dir.path()).await;
        assert!(out.contains("working tree clean"), "{out}");
    }

    #[tokio::test]
    async fn diff_query_returns_files_and_unified_hunks() {
        let repo = fixture_git_repo();
        std::fs::write(repo.path().join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(repo.path().join("new.txt"), "new\n").unwrap();

        let snapshot = diff_snapshot(repo.path()).await.unwrap();

        assert_eq!(snapshot.files[0].path, "a.txt");
        assert_eq!(snapshot.files[0].status, CoreDiffFileStatus::Modified);
        assert_eq!(snapshot.files[0].additions, 1);
        assert!(snapshot
            .files
            .iter()
            .any(|file| file.path == "new.txt" && file.status == CoreDiffFileStatus::Untracked));
        assert!(snapshot.unified.contains("+two"), "{}", snapshot.unified);
        assert!(snapshot.unified.contains("+new"), "{}", snapshot.unified);
    }

    #[tokio::test]
    async fn non_repo_returns_an_empty_structured_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let snapshot = diff_snapshot(dir.path()).await.unwrap();
        assert!(snapshot.files.is_empty());
        assert!(snapshot.unified.is_empty());
    }
}
