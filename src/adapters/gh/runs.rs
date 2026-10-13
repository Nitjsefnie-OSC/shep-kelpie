//! The logs of the CI runs that failed, read for a worker whose token cannot
//!
//! A worker on a repo kelpie's App covers holds a token that pushes and does
//! nothing else, so `gh run view` is refused it. Kelpie reads the failed
//! runs' logs as the login `gh` acts as and puts the end of each in the
//! worker's prompt.

use serde::Deserialize;

use super::{gh, unreadable};
use crate::ports::{BranchRun, ForgeError, Timestamp, Workflow, WorkflowState};
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
    for id in failed_runs(&listed, head, merge_group)?
        .into_iter()
        .take(RUNS)
    {
        text.push_str(&format!("--- run {id}\n{}\n", run_log_tail(repo, id)?));
    }
    Ok(text)
}

pub(super) fn run_log_tail(repo: &ForgeSlug, run: u64) -> Result<String, ForgeError> {
    let id = run.to_string();
    let log = gh(&["run", "view", &id, "--repo", repo.as_str(), "--log-failed"])?;
    Ok(failure_tail(&String::from_utf8_lossy(&log), LOG_BYTES))
}

/// The latest completed run of each workflow and event among the 50 newest completed runs on `branch`
pub(super) fn branch_runs(repo: &ForgeSlug, branch: &str) -> Result<Vec<BranchRun>, ForgeError> {
    let listed = gh(&[
        "run",
        "list",
        "--repo",
        repo.as_str(),
        "--branch",
        branch,
        "--status",
        "completed",
        "--limit",
        "50",
        "--json",
        "databaseId,workflowName,conclusion,headSha,event,createdAt",
    ])?;
    latest_runs(&listed)
}

/// How many workflows are asked for: a list this long may have been cut off
const WORKFLOWS: usize = 1000;

/// Every workflow of `repo`, the disabled ones too
pub(super) fn workflows(repo: &ForgeSlug) -> Result<Vec<Workflow>, ForgeError> {
    let limit = WORKFLOWS.to_string();
    let listed = gh(&[
        "workflow",
        "list",
        "--repo",
        repo.as_str(),
        "--all",
        "--limit",
        &limit,
        "--json",
        "name,path,state",
    ])?;
    let workflows = parse_workflows(&listed)?;
    if workflows.len() >= WORKFLOWS {
        return Err(ForgeError::Failed(format!(
            "{} workflows listed, which may not be all of them",
            workflows.len()
        )));
    }
    Ok(workflows)
}

// `state` is the field GitHub documents for a workflow, in "List repository
// workflows"; a value outside its documented ones is a list kelpie cannot read
fn parse_workflows(listed: &[u8]) -> Result<Vec<Workflow>, ForgeError> {
    #[derive(Deserialize)]
    struct Listed {
        name: String,
        path: String,
        state: String,
    }
    let workflows: Vec<Listed> = serde_json::from_slice(listed).map_err(|_| unreadable(listed))?;
    workflows
        .into_iter()
        .map(|w| {
            let state = match w.state.as_str() {
                "active" => WorkflowState::Active,
                "deleted" => WorkflowState::Deleted,
                "disabled_manually" => WorkflowState::DisabledManually,
                "disabled_inactivity" => WorkflowState::DisabledInactivity,
                "disabled_fork" => WorkflowState::DisabledFork,
                _ => return Err(unreadable(listed)),
            };
            Ok(Workflow {
                name: w.name,
                path: w.path,
                state,
            })
        })
        .collect()
}

// The newest run of each workflow and event in `listed`, in the order they
// first appear; of two runs created in the same second the one listed first stays
fn latest_runs(listed: &[u8]) -> Result<Vec<BranchRun>, ForgeError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Run {
        database_id: u64,
        workflow_name: String,
        conclusion: String,
        head_sha: String,
        event: String,
        created_at: String,
    }
    let runs: Vec<Run> = serde_json::from_slice(listed).map_err(|_| unreadable(listed))?;
    let mut latest: Vec<BranchRun> = Vec::new();
    for run in runs {
        let created = run
            .created_at
            .parse::<jiff::Timestamp>()
            .ok()
            .and_then(|at| u64::try_from(at.as_second()).ok())
            .map(Timestamp)
            .ok_or_else(|| unreadable(listed))?;
        let run = BranchRun {
            workflow: run.workflow_name,
            id: run.database_id,
            head: run.head_sha,
            event: run.event,
            conclusion: run.conclusion,
            created,
        };
        match (latest.iter_mut()).find(|r| r.workflow == run.workflow && r.event == run.event) {
            Some(kept) if kept.created < run.created => *kept = run,
            Some(_) => {}
            None => latest.push(run),
        }
    }
    Ok(latest)
}

/// Reruns the failed jobs of the failed runs of `head`
///
/// Refused where more runs failed than `failed_run_logs` reads, since the
/// unread logs may name a changed file and a run left red would make the
/// fix turn's "failed again" false. Every run is asked for, and the call
/// fails only when none was rerun: a run that did start is the rerun the
/// caller records, so a refusal of another run does not hide it.
pub(super) fn rerun_failed(repo: &ForgeSlug, head: &str) -> Result<(), ForgeError> {
    let listed = list(repo)?;
    let ids = failed_runs(&listed, head, false)?;
    if ids.is_empty() {
        return Err(ForgeError::Failed(format!("no failed run on {head}")));
    }
    if ids.len() > RUNS {
        return Err(ForgeError::Failed(format!(
            "{} runs failed on {head}, more than the {RUNS} whose logs kelpie reads",
            ids.len()
        )));
    }
    let mut first_error = None;
    let mut started = false;
    for id in ids {
        let id = id.to_string();
        match gh(&["run", "rerun", &id, "--repo", repo.as_str(), "--failed"]) {
            Ok(_) => started = true,
            Err(e) => first_error = first_error.or(Some(e)),
        }
    }
    match (started, first_error) {
        (false, Some(e)) => Err(e),
        _ => Ok(()),
    }
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

// Every failed run of `head`, or of the merge queue, newest first
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
        .collect())
}

// The last `max` bytes of `log` once cargo's progress lines, GitHub's
// timestamps and terminal colour codes are gone, so the end is the failure and
// not the build before it
fn failure_tail(log: &str, max: usize) -> String {
    let kept: Vec<String> = log
        .lines()
        .filter_map(|line| {
            // `gh run view --log-failed` puts the job and step before the text
            let mut parts = line.splitn(3, '\t');
            let (names, text) = match (parts.next(), parts.next(), parts.next()) {
                (Some(job), Some(step), Some(text)) => (format!("{job}\t{step}\t"), text),
                _ => (String::new(), line),
            };
            let text = strip_colour(strip_timestamp(text));
            (!is_progress(&text)).then(|| format!("{names}{text}"))
        })
        .collect();
    tail(&kept.join("\n"), max).to_owned()
}

// The verbs cargo prints as it builds, with the width it right-aligns them to
const PROGRESS_WIDTH: usize = 12;
const PROGRESS: [&str; 5] = [
    "Compiling",
    "Downloaded",
    "Downloading",
    "Checking",
    "Fresh",
];

// A line cargo prints as it builds: its verb right-aligned to 12 columns,
// then a crate and its version, or the `crates` a download counts. A test's
// own line that starts with the same word is not indented so, and stays.
fn is_progress(text: &str) -> bool {
    let word = text.trim_start_matches(' ');
    let indent = text.len() - word.len();
    PROGRESS.iter().any(|verb| {
        indent + verb.len() == PROGRESS_WIDTH
            && word
                .strip_prefix(verb)
                .and_then(|rest| rest.strip_prefix(' '))
                .is_some_and(|rest| {
                    rest.split(' ').any(|w| {
                        w == "crates"
                            || w.strip_prefix('v')
                                .is_some_and(|v| v.starts_with(|c: char| c.is_ascii_digit()))
                    })
                })
    })
}

// `text` without the `2026-10-13T10:00:00.0000000Z ` GitHub puts before it,
// and the byte order mark the first line of a log starts with
fn strip_timestamp(text: &str) -> &str {
    let text = text.trim_start_matches('\u{feff}');
    match text.split_once(' ') {
        Some((stamp, rest)) if is_timestamp(stamp) => rest,
        _ => text,
    }
}

// `2026-10-13T10:00:00.0000000Z`
fn is_timestamp(word: &str) -> bool {
    let b = word.as_bytes();
    b.len() >= 20
        && b.ends_with(b"Z")
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b[10] == b'T'
        && b[11..b.len() - 1]
            .iter()
            .all(|c| c.is_ascii_digit() || matches!(c, b':' | b'.'))
}

// `text` without its ANSI escape sequences, such as the colour cargo writes
// under `CARGO_TERM_COLOR=always`
fn strip_colour(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        if chars.next_if_eq(&'[').is_some() {
            // A sequence ends at its first byte from `@` to `~`
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        }
    }
    out
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
    fn every_failed_run_of_a_head_is_listed_not_only_the_ones_whose_logs_are_read() {
        let listed = br#"[
            {"conclusion":"failure","databaseId":4,"event":"pull_request","headSha":"abc"},
            {"conclusion":"failure","databaseId":3,"event":"pull_request","headSha":"abc"},
            {"conclusion":"failure","databaseId":2,"event":"pull_request","headSha":"abc"},
            {"conclusion":"failure","databaseId":1,"event":"pull_request","headSha":"abc"}
        ]"#;
        assert_eq!(failed_runs(listed, "abc", false).unwrap(), [4, 3, 2, 1]);
    }

    #[test]
    fn the_queues_failed_runs_are_those_of_the_merge_group_event() {
        assert_eq!(failed_runs(LISTED, "abc", true).unwrap(), [4]);
    }

    // `gh run list --branch main --status completed --limit 50 --json
    // databaseId,workflowName,conclusion,headSha,event,createdAt` on
    // shep-pm/shep with gh 2.96, its first fourteen runs: `file size`,
    // `Release` and `Release PR` ran on several heads, `test` failed on one and
    // passed on an older one, and `pages` was cancelled twice.
    const RECORDED: &[u8] = br#"[
        {"conclusion":"success","createdAt":"2026-10-13T10:58:41Z","databaseId":38387029879,"event":"push","headSha":"dc9b123d317baabe97afc3c22316f8a6723a9afe","workflowName":"file size"},
        {"conclusion":"success","createdAt":"2026-10-13T10:58:41Z","databaseId":38387029600,"event":"push","headSha":"dc9b123d317baabe97afc3c22316f8a6723a9afe","workflowName":"Release"},
        {"conclusion":"success","createdAt":"2026-10-13T10:57:51Z","databaseId":38386934120,"event":"push","headSha":"f0751a5563ba0a9c978af3330b05efa010e15172","workflowName":"file size"},
        {"conclusion":"success","createdAt":"2026-10-13T10:57:50Z","databaseId":38386933064,"event":"push","headSha":"f0751a5563ba0a9c978af3330b05efa010e15172","workflowName":"Release"},
        {"conclusion":"failure","createdAt":"2026-10-13T10:57:50Z","databaseId":38386933110,"event":"push","headSha":"f0751a5563ba0a9c978af3330b05efa010e15172","workflowName":"Release PR"},
        {"conclusion":"success","createdAt":"2026-10-13T10:43:27Z","databaseId":38385312112,"event":"push","headSha":"16d445f3b0a1b8ec446c7692f290d460f7837590","workflowName":"file size"},
        {"conclusion":"success","createdAt":"2026-10-13T10:43:27Z","databaseId":38385311813,"event":"push","headSha":"16d445f3b0a1b8ec446c7692f290d460f7837590","workflowName":"Release"},
        {"conclusion":"cancelled","createdAt":"2026-10-13T10:43:27Z","databaseId":38385311703,"event":"push","headSha":"16d445f3b0a1b8ec446c7692f290d460f7837590","workflowName":"pages"},
        {"conclusion":"failure","createdAt":"2026-10-13T10:43:26Z","databaseId":38385311310,"event":"push","headSha":"16d445f3b0a1b8ec446c7692f290d460f7837590","workflowName":"Release PR"},
        {"conclusion":"failure","createdAt":"2026-10-13T10:43:26Z","databaseId":38385311306,"event":"push","headSha":"16d445f3b0a1b8ec446c7692f290d460f7837590","workflowName":"test"},
        {"conclusion":"success","createdAt":"2026-10-13T10:28:04Z","databaseId":38383546750,"event":"push","headSha":"7665f5b080313743b4b569e4f93a6775328be708","workflowName":"file size"},
        {"conclusion":"success","createdAt":"2026-10-13T10:28:04Z","databaseId":38383546374,"event":"push","headSha":"7665f5b080313743b4b569e4f93a6775328be708","workflowName":"Release"},
        {"conclusion":"failure","createdAt":"2026-10-13T10:28:04Z","databaseId":38383546391,"event":"push","headSha":"7665f5b080313743b4b569e4f93a6775328be708","workflowName":"Release PR"},
        {"conclusion":"success","createdAt":"2026-10-13T10:28:04Z","databaseId":38383546340,"event":"push","headSha":"7665f5b080313743b4b569e4f93a6775328be708","workflowName":"test"},
        {"conclusion":"cancelled","createdAt":"2026-10-13T10:28:04Z","databaseId":38383546385,"event":"push","headSha":"7665f5b080313743b4b569e4f93a6775328be708","workflowName":"pages"}
    ]"#;

    fn run_of<'a>(runs: &'a [BranchRun], workflow: &str) -> &'a BranchRun {
        runs.iter().find(|r| r.workflow == workflow).unwrap()
    }

    #[test]
    fn a_workflows_latest_run_is_the_only_one_kept() {
        let runs = latest_runs(RECORDED).unwrap();

        let mut workflows: Vec<_> = runs.iter().map(|r| r.workflow.as_str()).collect();
        workflows.sort_unstable();
        assert_eq!(
            workflows,
            ["Release", "Release PR", "file size", "pages", "test"]
        );
        assert_eq!(
            run_of(&runs, "file size"),
            &BranchRun {
                workflow: "file size".to_owned(),
                id: 38_387_029_879,
                head: "dc9b123d317baabe97afc3c22316f8a6723a9afe".to_owned(),
                event: "push".to_owned(),
                conclusion: "success".to_owned(),
                created: Timestamp(1_791_889_121),
            }
        );
    }

    #[test]
    fn a_failed_run_is_kept_and_an_older_pass_of_its_workflow_is_not() {
        let runs = latest_runs(RECORDED).unwrap();

        let test = run_of(&runs, "test");
        assert_eq!(
            (test.id, test.conclusion.as_str()),
            (38_385_311_306, "failure")
        );
    }

    #[test]
    fn a_cancelled_run_is_kept_with_its_conclusion() {
        let runs = latest_runs(RECORDED).unwrap();

        let pages = run_of(&runs, "pages");
        assert_eq!(
            (pages.id, pages.conclusion.as_str()),
            (38_385_311_703, "cancelled")
        );
    }

    #[test]
    fn a_run_listed_before_a_newer_one_of_its_workflow_does_not_win_by_position() {
        let listed = br#"[
            {"conclusion":"failure","createdAt":"2026-10-13T10:00:00Z","databaseId":1,"event":"push","headSha":"a","workflowName":"test"},
            {"conclusion":"success","createdAt":"2026-10-13T11:00:00Z","databaseId":2,"event":"push","headSha":"b","workflowName":"test"}
        ]"#;

        let runs = latest_runs(listed).unwrap();

        assert_eq!(runs.iter().map(|r| r.id).collect::<Vec<_>>(), [2]);
    }

    #[test]
    fn a_pull_request_run_does_not_hide_the_push_run_of_the_same_workflow() {
        let listed = br#"[
            {"conclusion":"failure","createdAt":"2026-10-13T11:00:00Z","databaseId":2,"event":"pull_request","headSha":"b","workflowName":"test"},
            {"conclusion":"success","createdAt":"2026-10-13T10:00:00Z","databaseId":1,"event":"push","headSha":"a","workflowName":"test"}
        ]"#;

        let runs = latest_runs(listed).unwrap();

        assert_eq!(runs.iter().map(|r| r.id).collect::<Vec<_>>(), [2, 1]);
    }

    #[test]
    fn a_listing_gh_did_not_print_as_json_is_unreadable() {
        assert!(matches!(
            latest_runs(b"not json"),
            Err(ForgeError::Unreadable(_))
        ));
    }

    // What `gh workflow list --all --json name,path,state` printed on
    // shep-pm/shep with gh 2.96, where every workflow is active.
    const WORKFLOW_LIST: &[u8] = include_bytes!("../../../fixtures/gh-workflow-list.json");

    #[test]
    fn the_recorded_workflow_list_reads_as_active_workflows_with_their_paths() {
        let workflows = parse_workflows(WORKFLOW_LIST).unwrap();

        assert_eq!(workflows.len(), 11);
        assert!(workflows.iter().all(|w| w.state == WorkflowState::Active));
        assert_eq!(
            workflows[7],
            Workflow {
                name: "test".to_owned(),
                path: ".github/workflows/test.yml".to_owned(),
                state: WorkflowState::Active,
            }
        );
    }

    // Hand-written: no repo of the maintainer's has a deleted or disabled
    // workflow. The states are the ones GitHub documents for a workflow's
    // `state` field in "List repository workflows".
    #[test]
    fn every_state_github_documents_for_a_workflow_is_read() {
        let listed = br#"[
            {"name":"a","path":".github/workflows/a.yml","state":"active"},
            {"name":"b","path":".github/workflows/b.yml","state":"deleted"},
            {"name":"c","path":".github/workflows/c.yml","state":"disabled_manually"},
            {"name":"d","path":".github/workflows/d.yml","state":"disabled_inactivity"},
            {"name":"e","path":".github/workflows/e.yml","state":"disabled_fork"}
        ]"#;

        let states: Vec<_> = parse_workflows(listed)
            .unwrap()
            .into_iter()
            .map(|w| w.state)
            .collect();

        assert_eq!(
            states,
            [
                WorkflowState::Active,
                WorkflowState::Deleted,
                WorkflowState::DisabledManually,
                WorkflowState::DisabledInactivity,
                WorkflowState::DisabledFork,
            ]
        );
    }

    #[test]
    fn a_workflow_state_github_does_not_document_is_unreadable() {
        let listed = br#"[{"name":"a","path":"a.yml","state":"archived"}]"#;

        assert!(matches!(
            parse_workflows(listed),
            Err(ForgeError::Unreadable(_))
        ));
    }

    #[test]
    fn a_workflow_list_gh_did_not_print_as_json_is_unreadable() {
        assert!(matches!(
            parse_workflows(b"not json"),
            Err(ForgeError::Unreadable(_))
        ));
    }

    const STAMP: &str = "test\tBuild\t2026-10-13T10:00:00.0000000Z";

    #[test]
    fn a_failed_logs_tail_has_no_cargo_progress_lines() {
        let mut log = String::new();
        for n in 0..400 {
            log.push_str(&format!(
                "{STAMP}    Compiling crate{n} v0.1.0\n\
                 {STAMP}   Downloaded dep{n} v1.0.0\n\
                 {STAMP}  Downloading crates ...\n\
                 {STAMP}     Checking crate{n} v0.1.0\n\
                 {STAMP}        Fresh crate{n} v0.1.0\n"
            ));
        }
        log.push_str(&format!("{STAMP} error[E0308]: mismatched types\n"));
        log.push_str(&format!("{STAMP}  --> src/lib.rs:1:1\n"));

        let kept = failure_tail(&log, LOG_BYTES);

        assert_eq!(
            kept,
            "test\tBuild\terror[E0308]: mismatched types\ntest\tBuild\t --> src/lib.rs:1:1"
        );
    }

    #[test]
    fn coloured_progress_lines_are_dropped_and_colour_is_stripped_from_the_rest() {
        let log = format!(
            "{STAMP} \x1b[1m\x1b[92m   Compiling\x1b[0m foo v0.1.0\n\
             {STAMP} \x1b[1m\x1b[91merror\x1b[0m: could not compile `foo`\n"
        );

        assert_eq!(
            failure_tail(&log, LOG_BYTES),
            "test\tBuild\terror: could not compile `foo`"
        );
    }

    #[test]
    fn a_line_with_a_tab_in_its_text_loses_its_timestamp() {
        let log = format!("{STAMP} ---- a stdout ----\tx\n");

        assert_eq!(
            failure_tail(&log, LOG_BYTES),
            "test\tBuild\t---- a stdout ----\tx"
        );
    }

    #[test]
    fn a_failure_line_that_starts_with_a_progress_word_stays() {
        let log = format!(
            "{STAMP} Checking invariants after merge: left != right\n\
             {STAMP} Checking formatting...\n\
             {STAMP}   Downloading v1 of the fixture failed\n"
        );

        let kept = failure_tail(&log, LOG_BYTES);

        assert_eq!(kept.lines().count(), 3, "{kept}");
    }

    #[test]
    fn a_log_keeps_its_end_from_a_line_start_and_never_splits_a_character() {
        assert_eq!(tail("a\nb\n", 100), "a\nb");
        let log = "first line\nsecond line\nthird é line\n";
        assert_eq!(tail(log, 16), "third é line");
        assert_eq!(tail("é".repeat(10).as_str(), 5), "éé");
    }
}
