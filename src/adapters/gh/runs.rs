//! The logs of the CI runs that failed, read for a worker whose token cannot
//!
//! A worker on a repo kelpie's App covers holds a token that pushes and does
//! nothing else, so `gh run view` is refused it. Kelpie reads the failed
//! runs' logs as the login `gh` acts as and puts the end of each in the
//! worker's prompt.

use serde::Deserialize;

use super::{gh, unreadable};
use crate::ports::ForgeError;
use crate::settings::ForgeSlug;

/// How many failed runs are read
const RUNS: usize = 3;

/// How much of the end of each log is kept, in bytes: where a failure says why
const LOG_BYTES: usize = 6000;

pub(super) fn failed_run_logs(
    repo: &ForgeSlug,
    head: &str,
    merge_group: bool,
) -> Result<String, ForgeError> {
    let listed = list(repo)?;
    let mut text = String::new();
    for id in failed_runs(&listed, head, merge_group)? {
        let id = id.to_string();
        let log = gh(&["run", "view", &id, "--repo", repo.as_str(), "--log-failed"])?;
        text.push_str(&format!(
            "--- run {id}\n{}\n",
            tail(&String::from_utf8_lossy(&log), LOG_BYTES)
        ));
    }
    Ok(text)
}

/// Reruns the failed jobs of the failed runs of `head`
pub(super) fn rerun_failed(repo: &ForgeSlug, head: &str) -> Result<(), ForgeError> {
    let listed = list(repo)?;
    let ids = failed_runs(&listed, head, false)?;
    if ids.is_empty() {
        return Err(ForgeError::Failed(format!("no failed run on {head}")));
    }
    for id in ids {
        let id = id.to_string();
        gh(&["run", "rerun", &id, "--repo", repo.as_str(), "--failed"])?;
    }
    Ok(())
}

fn list(repo: &ForgeSlug) -> Result<Vec<u8>, ForgeError> {
    gh(&[
        "run",
        "list",
        "--repo",
        repo.as_str(),
        "--limit",
        "50",
        "--json",
        "databaseId,conclusion,headSha,event",
    ])
}

// The failed runs of `head`, or of the merge queue, newest first
fn failed_runs(listed: &[u8], head: &str, merge_group: bool) -> Result<Vec<u64>, ForgeError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Run {
        database_id: u64,
        conclusion: String,
        head_sha: String,
        event: String,
    }
    let runs: Vec<Run> = serde_json::from_slice(listed).map_err(|_| unreadable(listed))?;
    Ok(runs
        .into_iter()
        .filter(|r| r.conclusion == "failure")
        .filter(|r| match merge_group {
            true => r.event == "merge_group",
            false => r.head_sha == head,
        })
        .map(|r| r.database_id)
        .take(RUNS)
        .collect())
}

// The last `max` bytes of `log`, from the start of a line where one is near
fn tail(log: &str, max: usize) -> &str {
    if log.len() <= max {
        return log.trim_end();
    }
    let mut from = log.len() - max;
    while !log.is_char_boundary(from) {
        from += 1;
    }
    let cut = &log[from..];
    cut.split_once('\n')
        .map_or(cut, |(_, rest)| rest)
        .trim_end()
}

#[cfg(test)]
mod tests {
    use super::*;

    // The shape `gh run list --json databaseId,conclusion,headSha,event`
    // printed on this repo with gh 2.96, with conclusions and events varied.
    const LISTED: &[u8] = br#"[
        {"conclusion":"failure","databaseId":4,"event":"merge_group","headSha":"queue"},
        {"conclusion":"failure","databaseId":3,"event":"pull_request","headSha":"abc"},
        {"conclusion":"success","databaseId":2,"event":"pull_request","headSha":"abc"},
        {"conclusion":"failure","databaseId":1,"event":"pull_request","headSha":"old"}
    ]"#;

    #[test]
    fn a_pull_requests_failed_runs_are_those_of_its_head() {
        assert_eq!(failed_runs(LISTED, "abc", false).unwrap(), [3]);
    }

    #[test]
    fn the_queues_failed_runs_are_those_of_the_merge_group_event() {
        assert_eq!(failed_runs(LISTED, "abc", true).unwrap(), [4]);
    }

    #[test]
    fn a_log_keeps_its_end_from_a_line_start_and_never_splits_a_character() {
        assert_eq!(tail("a\nb\n", 100), "a\nb");
        let log = "first line\nsecond line\nthird é line\n";
        assert_eq!(tail(log, 16), "third é line");
        assert_eq!(tail("é".repeat(10).as_str(), 5), "éé");
    }
}
