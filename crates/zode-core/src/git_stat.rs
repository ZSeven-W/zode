//! Git working-tree modification stats for the sidebar "modified files"
//! section: which files differ from HEAD, with per-file added/removed line
//! counts. Read-only `git` subprocess calls; no libgit2 dependency.

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;

/// One modified path in the working tree. `added`/`removed` come from
/// `git diff HEAD --numstat`; they are `None` for untracked or binary files
/// (numstat prints `-` for binaries and skips untracked paths entirely).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitFileStat {
    /// Repo-root-relative path, as printed by `git status --porcelain`.
    pub path: String,
    pub added: Option<u32>,
    pub removed: Option<u32>,
}

/// All uncommitted changes (staged + unstaged + untracked) under `cwd`'s
/// repository, in `git status` order. `None` when `cwd` is not inside a git
/// work tree or `git` is unavailable — callers hide the section then.
pub async fn git_modified_files(cwd: &Path) -> Option<Vec<GitFileStat>> {
    let porcelain = git(cwd, &["status", "--porcelain"]).await?;
    // `diff HEAD` fails on an unborn branch (no commits yet); the list still
    // renders from porcelain, just without line counts.
    let numstat = git(cwd, &["diff", "HEAD", "--numstat"])
        .await
        .unwrap_or_default();
    Some(merge(parse_porcelain(&porcelain), parse_numstat(&numstat)))
}

/// Run one read-only git command; `None` on spawn failure or non-zero exit.
async fn git(cwd: &Path, args: &[&str]) -> Option<String> {
    let out = tokio::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Parse `git status --porcelain` (v1): `XY path`, `?? path`, renames as
/// `XY old -> new` (keep the new path). Quoted paths are unquoted but not
/// unescaped (good enough for display).
fn parse_porcelain(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|line| {
            if line.len() < 4 {
                return None;
            }
            let path = &line[3..];
            let path = path.rsplit(" -> ").next().unwrap_or(path);
            let path = path.trim_matches('"');
            (!path.is_empty()).then(|| path.to_string())
        })
        .collect()
}

/// Parse `git diff --numstat`: `added\tremoved\tpath`, `-` for binary.
fn parse_numstat(out: &str) -> HashMap<String, (Option<u32>, Option<u32>)> {
    out.lines()
        .filter_map(|line| {
            let mut cols = line.splitn(3, '\t');
            let added = cols.next()?.parse::<u32>().ok();
            let removed = cols.next()?.parse::<u32>().ok();
            let path = cols.next()?.trim_matches('"');
            (!path.is_empty()).then(|| (path.to_string(), (added, removed)))
        })
        .collect()
}

fn merge(
    paths: Vec<String>,
    counts: HashMap<String, (Option<u32>, Option<u32>)>,
) -> Vec<GitFileStat> {
    paths
        .into_iter()
        .map(|path| {
            let (added, removed) = counts.get(&path).copied().unwrap_or((None, None));
            GitFileStat {
                path,
                added,
                removed,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_parses_modified_untracked_and_renames() {
        let out = " M crates/a.rs\n?? new_file.rs\nR  old.rs -> renamed.rs\nA  \"sp ace.rs\"\n";
        assert_eq!(
            parse_porcelain(out),
            vec!["crates/a.rs", "new_file.rs", "renamed.rs", "sp ace.rs"]
        );
    }

    #[test]
    fn numstat_parses_counts_and_binary_dashes() {
        let out = "4\t1\tcrates/a.rs\n-\t-\tlogo.png\n";
        let map = parse_numstat(out);
        assert_eq!(map["crates/a.rs"], (Some(4), Some(1)));
        assert_eq!(map["logo.png"], (None, None));
    }

    #[test]
    fn merge_attaches_counts_and_leaves_untracked_bare() {
        let paths = vec!["a.rs".to_string(), "new.rs".to_string()];
        let mut counts = HashMap::new();
        counts.insert("a.rs".to_string(), (Some(7), Some(2)));
        let merged = merge(paths, counts);
        assert_eq!(merged[0].added, Some(7));
        assert_eq!(merged[0].removed, Some(2));
        assert_eq!(merged[1].path, "new.rs");
        assert_eq!(merged[1].added, None);
    }

    #[tokio::test]
    async fn non_git_dir_yields_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(git_modified_files(dir.path()).await.is_none());
    }
}
