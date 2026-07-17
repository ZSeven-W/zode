//! `gh`-backed pull-request status for the current branch - see
//! `docs/proposals/right-panel-parity.md` section 1.1.
//!
//! `fetch_pull_request_status` shells out to `git` (remote presence) and
//! the `gh` CLI (auth + PR status) through the injectable [`CommandRunner`]
//! seam, so tests can supply canned process output instead of needing real
//! `git`/`gh` binaries or network access.

use std::path::Path;
use std::process::Output;

use async_trait::async_trait;
use serde::Deserialize;
use zode_app_model::{ChecksState, PullRequestStatus};

/// Runs one child process and returns its output. The real implementation
/// shells out via `tokio::process::Command`; tests inject a fake.
#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(&self, program: &str, args: &[&str], cwd: &Path) -> std::io::Result<Output>;
}

/// Shells out for real via `tokio::process::Command`.
#[derive(Debug, Default)]
pub struct SystemCommandRunner;

#[async_trait]
impl CommandRunner for SystemCommandRunner {
    async fn run(&self, program: &str, args: &[&str], cwd: &Path) -> std::io::Result<Output> {
        tokio::process::Command::new(program)
            .args(args)
            .current_dir(cwd)
            .output()
            .await
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PrView {
    number: u64,
    state: String,
    mergeable: Option<String>,
    #[serde(default)]
    status_check_rollup: Vec<CheckRollupEntry>,
}

#[derive(Debug, Deserialize)]
struct CheckRollupEntry {
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    conclusion: Option<String>,
}

/// Fetches the current pull-request status for the repository at `cwd`.
/// See [`PullRequestStatus`] for the full taxonomy this produces.
pub async fn fetch_pull_request_status(
    runner: &dyn CommandRunner,
    cwd: &Path,
) -> PullRequestStatus {
    if !has_git_remote(runner, cwd).await {
        return PullRequestStatus::NoRemote;
    }
    match runner.run("gh", &["--version"], cwd).await {
        Ok(output) if output.status.success() => {}
        _ => return PullRequestStatus::GhCliUnavailable,
    }
    match runner.run("gh", &["auth", "status"], cwd).await {
        Ok(output) if output.status.success() => {}
        Ok(_) => return PullRequestStatus::GhCliSignedOut,
        Err(_) => return PullRequestStatus::GhCliUnavailable,
    }
    let output = match runner
        .run(
            "gh",
            &[
                "pr",
                "view",
                "--json",
                "state,statusCheckRollup,mergeable,number",
            ],
            cwd,
        )
        .await
    {
        Ok(output) => output,
        Err(_) => return PullRequestStatus::Unavailable,
    };
    if !output.status.success() {
        // `gh pr view` exits non-zero (typically "no pull requests found
        // for branch ...") when the current branch has no open PR - the
        // common, expected case, not a fetch failure.
        return PullRequestStatus::NoPr;
    }
    let Ok(view) = serde_json::from_slice::<PrView>(&output.stdout) else {
        return PullRequestStatus::Unavailable;
    };
    if !view.state.eq_ignore_ascii_case("OPEN") {
        return PullRequestStatus::NoPr;
    }
    if view.mergeable.as_deref() == Some("CONFLICTING") {
        return PullRequestStatus::MergeConflicts {
            number: view.number,
        };
    }
    PullRequestStatus::Pr {
        number: view.number,
        checks: checks_state(&view.status_check_rollup),
    }
}

async fn has_git_remote(runner: &dyn CommandRunner, cwd: &Path) -> bool {
    matches!(
        runner.run("git", &["remote"], cwd).await,
        Ok(output) if output.status.success() && !output.stdout.iter().all(u8::is_ascii_whitespace)
    )
}

fn checks_state(rollup: &[CheckRollupEntry]) -> ChecksState {
    if rollup.is_empty() {
        return ChecksState::None;
    }
    let mut pending = false;
    for entry in rollup {
        let status = entry.status.as_deref().unwrap_or_default();
        if !status.eq_ignore_ascii_case("COMPLETED") {
            pending = true;
            continue;
        }
        let conclusion = entry.conclusion.as_deref().unwrap_or_default();
        if !conclusion.eq_ignore_ascii_case("SUCCESS")
            && !conclusion.eq_ignore_ascii_case("NEUTRAL")
        {
            return ChecksState::Failing;
        }
    }
    if pending {
        ChecksState::Pending
    } else {
        ChecksState::Successful
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;
    use std::sync::Mutex;

    #[derive(Default)]
    struct FakeRunner {
        responses: Mutex<HashMap<String, std::io::Result<Output>>>,
    }

    fn ok(stdout: &str, success: bool) -> std::io::Result<Output> {
        Ok(Output {
            status: ExitStatus::from_raw(i32::from(!success)),
            stdout: stdout.as_bytes().to_vec(),
            stderr: Vec::new(),
        })
    }

    impl FakeRunner {
        fn set(&self, program: &str, args: &[&str], response: std::io::Result<Output>) {
            self.responses
                .lock()
                .unwrap()
                .insert(key(program, args), response);
        }
    }

    fn key(program: &str, args: &[&str]) -> String {
        format!("{program} {}", args.join(" "))
    }

    #[async_trait]
    impl CommandRunner for FakeRunner {
        async fn run(&self, program: &str, args: &[&str], _cwd: &Path) -> std::io::Result<Output> {
            match self.responses.lock().unwrap().remove(&key(program, args)) {
                Some(Ok(output)) => Ok(output),
                Some(Err(error)) => Err(error),
                None => panic!("unexpected command: {}", key(program, args)),
            }
        }
    }

    fn base_runner() -> FakeRunner {
        let runner = FakeRunner::default();
        runner.set("git", &["remote"], ok("origin\n", true));
        runner.set("gh", &["--version"], ok("gh version 2.0.0", true));
        runner.set("gh", &["auth", "status"], ok("", true));
        runner
    }

    #[tokio::test]
    async fn no_remote_short_circuits_before_any_gh_call() {
        let runner = FakeRunner::default();
        runner.set("git", &["remote"], ok("", true));
        let status = fetch_pull_request_status(&runner, Path::new(".")).await;
        assert_eq!(status, PullRequestStatus::NoRemote);
    }

    #[tokio::test]
    async fn missing_gh_binary_is_unavailable() {
        let runner = FakeRunner::default();
        runner.set("git", &["remote"], ok("origin\n", true));
        runner.set(
            "gh",
            &["--version"],
            Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
        );
        let status = fetch_pull_request_status(&runner, Path::new(".")).await;
        assert_eq!(status, PullRequestStatus::GhCliUnavailable);
    }

    #[tokio::test]
    async fn signed_out_gh_reports_signed_out() {
        let runner = base_runner();
        runner.set("gh", &["auth", "status"], ok("", false));
        let status = fetch_pull_request_status(&runner, Path::new(".")).await;
        assert_eq!(status, PullRequestStatus::GhCliSignedOut);
    }

    #[tokio::test]
    async fn no_open_pr_maps_to_no_pr() {
        let runner = base_runner();
        runner.set(
            "gh",
            &[
                "pr",
                "view",
                "--json",
                "state,statusCheckRollup,mergeable,number",
            ],
            ok("", false),
        );
        let status = fetch_pull_request_status(&runner, Path::new(".")).await;
        assert_eq!(status, PullRequestStatus::NoPr);
    }

    #[tokio::test]
    async fn conflicting_pr_maps_to_merge_conflicts() {
        let runner = base_runner();
        runner.set(
            "gh",
            &[
                "pr",
                "view",
                "--json",
                "state,statusCheckRollup,mergeable,number",
            ],
            ok(
                r#"{"number":42,"state":"OPEN","mergeable":"CONFLICTING","statusCheckRollup":[]}"#,
                true,
            ),
        );
        let status = fetch_pull_request_status(&runner, Path::new(".")).await;
        assert_eq!(status, PullRequestStatus::MergeConflicts { number: 42 });
    }

    #[tokio::test]
    async fn all_checks_passing_is_successful() {
        let runner = base_runner();
        runner.set(
            "gh",
            &[
                "pr",
                "view",
                "--json",
                "state,statusCheckRollup,mergeable,number",
            ],
            ok(
                r#"{"number":7,"state":"OPEN","mergeable":"MERGEABLE","statusCheckRollup":[
                    {"status":"COMPLETED","conclusion":"SUCCESS"}
                ]}"#,
                true,
            ),
        );
        let status = fetch_pull_request_status(&runner, Path::new(".")).await;
        assert_eq!(
            status,
            PullRequestStatus::Pr {
                number: 7,
                checks: ChecksState::Successful
            }
        );
    }

    #[tokio::test]
    async fn a_failing_check_wins_over_pending_ones() {
        let runner = base_runner();
        runner.set(
            "gh",
            &[
                "pr",
                "view",
                "--json",
                "state,statusCheckRollup,mergeable,number",
            ],
            ok(
                r#"{"number":7,"state":"OPEN","mergeable":"MERGEABLE","statusCheckRollup":[
                    {"status":"IN_PROGRESS","conclusion":null},
                    {"status":"COMPLETED","conclusion":"FAILURE"}
                ]}"#,
                true,
            ),
        );
        let status = fetch_pull_request_status(&runner, Path::new(".")).await;
        assert_eq!(
            status,
            PullRequestStatus::Pr {
                number: 7,
                checks: ChecksState::Failing
            }
        );
    }

    #[tokio::test]
    async fn only_pending_checks_report_pending() {
        let runner = base_runner();
        runner.set(
            "gh",
            &[
                "pr",
                "view",
                "--json",
                "state,statusCheckRollup,mergeable,number",
            ],
            ok(
                r#"{"number":7,"state":"OPEN","mergeable":"MERGEABLE","statusCheckRollup":[
                    {"status":"QUEUED","conclusion":null}
                ]}"#,
                true,
            ),
        );
        let status = fetch_pull_request_status(&runner, Path::new(".")).await;
        assert_eq!(
            status,
            PullRequestStatus::Pr {
                number: 7,
                checks: ChecksState::Pending
            }
        );
    }

    #[tokio::test]
    async fn a_closed_pr_reads_as_no_pr() {
        let runner = base_runner();
        runner.set(
            "gh",
            &[
                "pr",
                "view",
                "--json",
                "state,statusCheckRollup,mergeable,number",
            ],
            ok(
                r#"{"number":7,"state":"MERGED","mergeable":"UNKNOWN","statusCheckRollup":[]}"#,
                true,
            ),
        );
        let status = fetch_pull_request_status(&runner, Path::new(".")).await;
        assert_eq!(status, PullRequestStatus::NoPr);
    }
}
