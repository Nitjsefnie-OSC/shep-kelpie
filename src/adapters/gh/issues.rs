//! Issues kelpie files: the open ones it checks first, a new one, and a
//! parent it closes

use serde::Deserialize;

use super::{gh, unreadable};
use crate::ports::{FinishedParent, ForgeError, OpenIssue, SubIssue, SubIssueState};
use crate::settings::ForgeSlug;

// The same ceiling the board's lists use, since `gh` lists newest first and
// stops at its limit.
const LIST_LIMIT: &str = "1000";

pub(super) fn open_issues(repo: &ForgeSlug) -> Result<Vec<OpenIssue>, ForgeError> {
    let args = [
        "issue",
        "list",
        "--repo",
        repo.as_str(),
        "--state",
        "open",
        "--limit",
        LIST_LIMIT,
        "--json",
        "number,title,body",
    ];
    parse_open_issues(&gh(&args)?)
}

// `gh` counts an issue's sub-issues closed whatever they closed as, so this
// names the candidates and `sub_issues` tells which of them built something.
pub(super) fn finished_parents(repo: &ForgeSlug) -> Result<Vec<FinishedParent>, ForgeError> {
    let args = [
        "issue",
        "list",
        "--repo",
        repo.as_str(),
        "--state",
        "open",
        "--limit",
        LIST_LIMIT,
        "--json",
        "number,subIssuesSummary",
    ];
    parse_finished_parents(&gh(&args)?)
}

fn parse_finished_parents(stdout: &[u8]) -> Result<Vec<FinishedParent>, ForgeError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Listed {
        number: u64,
        sub_issues_summary: Summary,
    }
    #[derive(Deserialize)]
    struct Summary {
        total: u64,
        completed: u64,
    }
    let listed: Vec<Listed> = serde_json::from_slice(stdout).map_err(|_| unreadable(stdout))?;
    Ok(listed
        .into_iter()
        .filter(|i| i.sub_issues_summary.total > 0)
        .filter(|i| i.sub_issues_summary.completed >= i.sub_issues_summary.total)
        .map(|i| FinishedParent {
            number: i.number,
            sub_issues: i.sub_issues_summary.total,
        })
        .collect())
}

pub(super) fn create_issue(
    repo: &ForgeSlug,
    title: &str,
    body: &str,
    labels: &[&str],
) -> Result<u64, ForgeError> {
    let mut args = vec![
        "issue",
        "create",
        "--repo",
        repo.as_str(),
        "--title",
        title,
        "--body",
        body,
    ];
    for label in labels {
        args.extend(["--label", label]);
    }
    parse_created(&gh(&args)?)
}

pub(super) fn set_label(
    repo: &ForgeSlug,
    number: u64,
    label: &str,
    add: bool,
) -> Result<(), ForgeError> {
    let flag = if add { "--add-label" } else { "--remove-label" };
    let number = number.to_string();
    gh(&[
        "issue",
        "edit",
        &number,
        "--repo",
        repo.as_str(),
        flag,
        label,
    ])
    .map(drop)
}

fn parse_open_issues(stdout: &[u8]) -> Result<Vec<OpenIssue>, ForgeError> {
    #[derive(Deserialize)]
    struct Listed {
        number: u64,
        title: String,
        body: Option<String>,
    }
    let listed: Vec<Listed> = serde_json::from_slice(stdout).map_err(|_| unreadable(stdout))?;
    Ok(listed
        .into_iter()
        .map(|issue| OpenIssue {
            number: issue.number,
            title: issue.title,
            body: issue.body.unwrap_or_default(),
        })
        .collect())
}

pub(super) fn close_issue(repo: &ForgeSlug, number: u64) -> Result<(), ForgeError> {
    let number = number.to_string();
    gh(&["issue", "close", &number, "--repo", repo.as_str()]).map(drop)
}

// GitHub allows a parent 100 sub-issues, so one page lists them all.
pub(super) fn sub_issues(repo: &ForgeSlug, parent: u64) -> Result<Vec<SubIssue>, ForgeError> {
    let path = format!(
        "repos/{}/issues/{parent}/sub_issues?per_page=100",
        repo.as_str()
    );
    parse_sub_issues(&gh(&["api", &path])?)
}

fn parse_sub_issues(stdout: &[u8]) -> Result<Vec<SubIssue>, ForgeError> {
    #[derive(Deserialize)]
    struct Listed {
        number: u64,
        state: String,
        state_reason: Option<String>,
    }
    let listed: Vec<Listed> = serde_json::from_slice(stdout).map_err(|_| unreadable(stdout))?;
    Ok(listed
        .into_iter()
        .map(|sub| SubIssue {
            number: sub.number,
            state: match (sub.state.as_str(), sub.state_reason.as_deref()) {
                ("closed", Some("not_planned" | "duplicate")) => SubIssueState::NotPlanned,
                ("closed", _) => SubIssueState::Completed,
                _ => SubIssueState::Open,
            },
        })
        .collect())
}

// `gh issue create` prints the new issue's URL, which ends in its number.
fn parse_created(stdout: &[u8]) -> Result<u64, ForgeError> {
    let text = String::from_utf8_lossy(stdout);
    text.trim()
        .rsplit('/')
        .next()
        .and_then(|number| number.parse().ok())
        .ok_or_else(|| unreadable(stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finished_parents_are_the_issues_whose_sub_issues_are_all_closed() {
        let out = br#"[
            {"number":1,"subIssuesSummary":{"total":0,"completed":0,"percentCompleted":0}},
            {"number":2,"subIssuesSummary":{"total":2,"completed":1,"percentCompleted":50}},
            {"number":3,"subIssuesSummary":{"total":2,"completed":2,"percentCompleted":100}}
        ]"#;
        assert_eq!(
            parse_finished_parents(out).unwrap(),
            [FinishedParent {
                number: 3,
                sub_issues: 2
            }]
        );
    }

    #[test]
    fn open_issues_are_read_with_a_body_that_may_be_null() {
        let out = br#"[{"number":4,"title":"a","body":"b"},{"number":5,"title":"c","body":null}]"#;
        let issues = parse_open_issues(out).unwrap();
        assert_eq!(
            issues,
            [
                OpenIssue {
                    number: 4,
                    title: "a".into(),
                    body: "b".into()
                },
                OpenIssue {
                    number: 5,
                    title: "c".into(),
                    body: String::new()
                },
            ]
        );
    }

    #[test]
    fn sub_issues_are_read_with_the_reason_each_closed_for() {
        let out = br#"[
            {"number":1,"state":"open","state_reason":"reopened"},
            {"number":2,"state":"closed","state_reason":"completed"},
            {"number":3,"state":"closed","state_reason":"not_planned"},
            {"number":4,"state":"closed","state_reason":"duplicate"},
            {"number":5,"state":"closed","state_reason":null}
        ]"#;
        let states: Vec<_> = parse_sub_issues(out)
            .unwrap()
            .iter()
            .map(|s| s.state)
            .collect();
        assert_eq!(
            states,
            [
                SubIssueState::Open,
                SubIssueState::Completed,
                SubIssueState::NotPlanned,
                SubIssueState::NotPlanned,
                SubIssueState::Completed,
            ]
        );
    }

    #[test]
    fn a_created_issues_number_is_the_end_of_the_url_gh_prints() {
        let out = b"https://github.com/shep-pm/shep-kelpie/issues/143\n";
        assert_eq!(parse_created(out).unwrap(), 143);
    }

    #[test]
    fn output_with_no_number_is_unreadable() {
        assert!(matches!(
            parse_created(b"created it\n"),
            Err(ForgeError::Unreadable(_))
        ));
    }
}
