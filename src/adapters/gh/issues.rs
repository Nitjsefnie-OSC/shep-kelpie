//! Issues kelpie files: the open ones it checks first, a new one, and a
//! parent it closes

use serde::Deserialize;

use super::review_bot::time;
use super::{gh, unreadable};
use crate::ports::{
    FinishedParent, ForgeError, IssueCloseReason, OpenIssue, OpenPull, SubIssue, SubIssueState,
};
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
        "number,title,body,author,labels,createdAt",
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
    #[serde(rename_all = "camelCase")]
    struct Listed {
        number: u64,
        title: String,
        body: Option<String>,
        author: Option<Author>,
        labels: Vec<Label>,
        created_at: String,
    }
    #[derive(Deserialize)]
    struct Author {
        login: String,
        is_bot: bool,
    }
    #[derive(Deserialize)]
    struct Label {
        name: String,
    }
    let listed: Vec<Listed> = serde_json::from_slice(stdout).map_err(|_| unreadable(stdout))?;
    listed
        .into_iter()
        .map(|issue| {
            Ok(OpenIssue {
                number: issue.number,
                title: issue.title,
                body: issue.body.unwrap_or_default(),
                author_is_bot: issue.author.as_ref().is_some_and(|a| a.is_bot),
                author: issue.author.map(|a| a.login).unwrap_or_default(),
                labels: issue.labels.into_iter().map(|l| l.name).collect(),
                created: time(&issue.created_at, stdout)?,
            })
        })
        .collect()
}

pub(super) fn close_issue(repo: &ForgeSlug, number: u64) -> Result<(), ForgeError> {
    let number = number.to_string();
    gh(&["issue", "close", &number, "--repo", repo.as_str()]).map(drop)
}

// `gh issue close` has no flag for the reason an issue closed for, so the
// issue is closed through the API, which records `state_reason` on it.
fn close_as_args(repo: &ForgeSlug, number: u64, reason: IssueCloseReason) -> Vec<String> {
    let path = format!("repos/{}/issues/{number}", repo.as_str());
    let why = format!("state_reason={}", reason.as_str());
    [
        "api",
        "--method",
        "PATCH",
        path.as_str(),
        "-f",
        "state=closed",
        "-f",
        why.as_str(),
    ]
    .map(str::to_owned)
    .to_vec()
}

pub(super) fn close_issue_as(
    repo: &ForgeSlug,
    number: u64,
    reason: IssueCloseReason,
) -> Result<(), ForgeError> {
    let args = close_as_args(repo, number, reason);
    gh(&args.iter().map(String::as_str).collect::<Vec<_>>()).map(drop)
}

pub(super) fn open_pulls(repo: &ForgeSlug) -> Result<Vec<OpenPull>, ForgeError> {
    let args = [
        "pr",
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
    parse_open_pulls(&gh(&args)?)
}

fn parse_open_pulls(stdout: &[u8]) -> Result<Vec<OpenPull>, ForgeError> {
    #[derive(Deserialize)]
    struct Listed {
        number: u64,
        title: String,
        body: Option<String>,
    }
    let listed: Vec<Listed> = serde_json::from_slice(stdout).map_err(|_| unreadable(stdout))?;
    Ok(listed
        .into_iter()
        .map(|pull| OpenPull {
            number: pull.number,
            title: pull.title,
            closes: closes_named_in(pull.body.as_deref().unwrap_or_default()),
        })
        .collect())
}

// The issues a pull request's body names with `Resolves`, `Fixes` or `Closes`,
// each keyword read in any case, in the order they first appear. As GitHub
// reads them, the keyword is a word of its own: it may carry a colon after it
// (`Fixes: #12`, `Fixes:#12`) and markup or punctuation before it (`(Fixes
// #12)`, `**Fixes #12**`), and a number ends at whitespace, punctuation or the
// end of the body: `#12abc` and `#12_3` name nothing, since GitHub reads no
// issue of that number.
fn closes_named_in(body: &str) -> Vec<u64> {
    const KEYWORDS: [&str; 3] = ["resolves", "fixes", "closes"];
    let words: Vec<&str> = body.split_whitespace().collect();
    let mut named = Vec::new();
    for (at, word) in words.iter().enumerate() {
        let word = word.trim_start_matches(|c: char| !c.is_alphanumeric() && c != '_');
        let (keyword, tail) = match word.split_once(':') {
            Some((keyword, tail)) => (keyword, Some(tail)),
            None => (word, None),
        };
        if !KEYWORDS.iter().any(|k| keyword.eq_ignore_ascii_case(k)) {
            continue;
        }
        // The number is what the colon left in the same word, or the next word
        let tail = tail.filter(|t| !t.is_empty());
        let rest = match tail {
            Some(tail) => tail,
            None => words.get(at + 1).map_or("", |next| next),
        };
        let Some(rest) = rest.strip_prefix('#') else {
            continue;
        };
        let digits: String = (rest.chars())
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        let ends = match rest.chars().nth(digits.len()) {
            None => true,
            Some(next) => !(next.is_ascii_alphanumeric() || matches!(next, '_' | '-')),
        };
        let Ok(number) = digits.parse::<u64>() else {
            continue;
        };
        if !ends || digits.is_empty() {
            continue;
        }
        if !named.contains(&number) {
            named.push(number);
        }
    }
    named
}

pub(super) fn close_pull_request(repo: &ForgeSlug, number: u64) -> Result<(), ForgeError> {
    let number = number.to_string();
    gh(&["pr", "close", &number, "--repo", repo.as_str()]).map(drop)
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
    use crate::ports::Timestamp;

    #[test]
    fn an_issue_closes_with_the_reason_the_forge_records_it_under() {
        let repo = ForgeSlug::try_from("shep-pm/koji".to_owned()).unwrap();
        for (reason, named) in [
            (IssueCloseReason::Duplicate, "duplicate"),
            (IssueCloseReason::NotPlanned, "not_planned"),
            (IssueCloseReason::Completed, "completed"),
        ] {
            let why = format!("state_reason={named}");
            assert_eq!(
                close_as_args(&repo, 12, reason),
                [
                    "api",
                    "--method",
                    "PATCH",
                    "repos/shep-pm/koji/issues/12",
                    "-f",
                    "state=closed",
                    "-f",
                    why.as_str(),
                ],
                "{reason:?}"
            );
        }
    }

    #[test]
    fn open_pulls_are_read_with_the_issues_their_bodies_close() {
        let out = br#"[
            {"number":41,"title":"Fix the crash","body":"One line.\n\nResolves #30\nfixes #31, and Closes #30 again."},
            {"number":42,"title":"Rework the docs","body":"No closing line here."},
            {"number":43,"title":"No body","body":null},
            {"number":44,"title":"Stop the crash","body":"Closes: #30, and (fixes #31)."}
        ]"#;
        let pulls = parse_open_pulls(out).unwrap();
        assert_eq!(
            pulls,
            [
                OpenPull {
                    number: 41,
                    title: "Fix the crash".into(),
                    closes: vec![30, 31],
                },
                OpenPull {
                    number: 42,
                    title: "Rework the docs".into(),
                    closes: Vec::new(),
                },
                OpenPull {
                    number: 43,
                    title: "No body".into(),
                    closes: Vec::new(),
                },
                OpenPull {
                    number: 44,
                    title: "Stop the crash".into(),
                    closes: vec![30, 31],
                },
            ]
        );
    }

    #[test]
    fn a_closing_keyword_names_nothing_without_a_number_after_it() {
        for body in [
            "Resolves #<issue>",
            "Resolves the crash",
            "fixes issue 12",
            "Closes#12",
            "Fixes:: #12",
            "Fixes::#12",
            "Fixes. #12",
            "unfixes #12",
            "Resolves #12abc",
            "Resolves #12_3",
            "Resolves #12-13",
            "Resolves #1x",
            "Resolves #",
            "Resolves:",
        ] {
            assert_eq!(closes_named_in(body), Vec::<u64>::new(), "{body}");
        }
        assert_eq!(closes_named_in("Fixes #12.\n"), [12]);
        assert_eq!(closes_named_in("Resolves #12; fixes #13."), [12, 13]);
    }

    // GitHub reads the keyword at a word boundary and takes a colon between it
    // and the number, with or without a space, so a body written either way
    // names the issue, and markup around the keyword hides nothing.
    #[test]
    fn a_closing_keyword_names_the_number_after_a_colon_or_under_punctuation() {
        for body in [
            "Fixes: #12",
            "Fixes:#12",
            "Closes: #12",
            "(Resolves #12)",
            "- Fixes: #12",
            "`Closes #12`",
            "**Closes #12**",
        ] {
            assert_eq!(closes_named_in(body), [12], "{body}");
        }
        assert_eq!(closes_named_in("Resolves: #12 and (fixes #13)"), [12, 13]);
    }

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
        let out = br#"[
            {"number":4,"title":"a","body":"b","createdAt":"2026-10-14T08:00:00Z",
             "author":{"login":"rin","is_bot":false,"id":"U_1","name":"Rin"},
             "labels":[{"name":"bug","id":"L_1"},{"name":"needs-info","id":"L_2"}]},
            {"number":5,"title":"c","body":null,"createdAt":"2026-10-14T09:30:00Z",
             "author":{"login":"app/kelpie","is_bot":true},"labels":[]},
            {"number":6,"title":"d","body":"","createdAt":"2026-10-14T09:30:00Z",
             "author":null,"labels":[]}
        ]"#;
        let issues = parse_open_issues(out).unwrap();
        assert_eq!(
            issues,
            [
                OpenIssue {
                    number: 4,
                    title: "a".into(),
                    body: "b".into(),
                    author: "rin".into(),
                    author_is_bot: false,
                    labels: vec!["bug".into(), "needs-info".into()],
                    created: Timestamp(1_791_964_800),
                },
                OpenIssue {
                    number: 5,
                    title: "c".into(),
                    body: String::new(),
                    author: "app/kelpie".into(),
                    author_is_bot: true,
                    labels: Vec::new(),
                    created: Timestamp(1_791_970_200),
                },
                OpenIssue {
                    number: 6,
                    title: "d".into(),
                    body: String::new(),
                    author: String::new(),
                    author_is_bot: false,
                    labels: Vec::new(),
                    created: Timestamp(1_791_970_200),
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
