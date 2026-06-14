//! Working-tree diff for the `/diff` command. Shells out to the system `git`
//! (matching `tools::git`) and returns a combined unstaged + staged diff as
//! plain text, ready to push into the chat view.

use std::path::Path;

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

    #[tokio::test]
    async fn non_repo_dir_reports_clean() {
        let dir = tempfile::tempdir().unwrap();
        let out = working_tree_diff(dir.path()).await;
        assert!(out.contains("working tree clean"), "{out}");
    }
}
