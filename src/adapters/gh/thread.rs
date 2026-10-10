//! The comments written on a repo's threads, and who may answer a ruling in them

use serde::Deserialize;

use super::{gh, unreadable};
use crate::ports::{ForgeError, ThreadComment, Timestamp};
use crate::settings::ForgeSlug;

// One page is as many comments as GitHub gives.
const PER_PAGE: &str = "per_page=100";

pub(super) fn comments_since(
    repo: &ForgeSlug,
    since: Timestamp,
) -> Result<Vec<ThreadComment>, ForgeError> {
    let since = i64::try_from(since.0)
        .ok()
        .and_then(|s| jiff::Timestamp::from_second(s).ok())
        .ok_or_else(|| ForgeError::Unreadable(format!("a time out of range: {}", since.0)))?;
    let path = format!(
        "repos/{}/issues/comments?{PER_PAGE}&sort=created&direction=asc&since={since}",
        repo.as_str()
    );
    parse_comments(&gh(&["api", "--paginate", "--slurp", &path])?)
}

pub(super) fn can_write(repo: &ForgeSlug, login: &str) -> Result<bool, ForgeError> {
    let path = format!("repos/{}/collaborators/{login}/permission", repo.as_str());
    match gh(&["api", &path]) {
        Ok(stdout) => parse_permission(&stdout),
        // A login that is no user, or an App's, is no collaborator.
        Err(ForgeError::Failed(why)) if why.contains("HTTP 404") => Ok(false),
        Err(e) => Err(e),
    }
}

// The pages `gh api --paginate --slurp` gathers, each GitHub's list of issue comments.
fn parse_comments(stdout: &[u8]) -> Result<Vec<ThreadComment>, ForgeError> {
    #[derive(Deserialize)]
    struct Comment {
        id: u64,
        issue_url: String,
        user: Option<User>,
        body: Option<String>,
        created_at: String,
    }
    #[derive(Deserialize)]
    struct User {
        login: String,
        r#type: String,
    }
    let pages: Vec<Vec<Comment>> =
        serde_json::from_slice(stdout).map_err(|_| unreadable(stdout))?;
    pages
        .into_iter()
        .flatten()
        .map(|comment| {
            let thread = comment
                .issue_url
                .rsplit('/')
                .next()
                .and_then(|n| n.parse().ok());
            let created = comment.created_at.parse::<jiff::Timestamp>().ok();
            let created = created.and_then(|at| u64::try_from(at.as_second()).ok());
            let (Some(thread), Some(created)) = (thread, created) else {
                return Err(unreadable(stdout));
            };
            // A deleted account is a ghost, which answers nothing.
            let (author, bot) = match comment.user {
                Some(user) => (user.login, user.r#type != "User"),
                None => ("ghost".to_owned(), true),
            };
            Ok(ThreadComment {
                id: comment.id,
                thread,
                author,
                bot,
                body: comment.body.unwrap_or_default(),
                created: Timestamp(created),
            })
        })
        .collect()
}

// `permission` is `admin`, `write`, `read` or `none`; the maintain role reads as write
// and triage as read.
fn parse_permission(stdout: &[u8]) -> Result<bool, ForgeError> {
    #[derive(Deserialize)]
    struct Permission {
        permission: String,
    }
    let said: Permission = serde_json::from_slice(stdout).map_err(|_| unreadable(stdout))?;
    match said.permission.as_str() {
        "admin" | "write" => Ok(true),
        "read" | "none" => Ok(false),
        _ => Err(unreadable(stdout)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // GitHub's documented answer to `GET /repos/{owner}/{repo}/issues/comments`,
    // as `--paginate --slurp` gathers two pages of it, trimmed to the fields
    // kelpie reads and one it ignores. A worker's sandbox cannot run `gh api`,
    // so this is the documented shape rather than a recording.
    const PAGES: &str = r#"[
      [
        {"id": 1, "issue_url": "https://api.github.com/repos/o/r/issues/71",
         "user": {"login": "octocat", "type": "User", "id": 1},
         "body": "yes", "created_at": "2026-10-10T05:31:54Z"}
      ],
      [
        {"id": 2, "issue_url": "https://api.github.com/repos/o/r/issues/7",
         "user": {"login": "kelpie-o[bot]", "type": "Bot"},
         "body": "Merge it?", "created_at": "2026-10-10T05:32:00Z"},
        {"id": 3, "issue_url": "https://api.github.com/repos/o/r/issues/7",
         "user": null, "body": null, "created_at": "2026-10-10T05:33:00Z"}
      ]
    ]"#;

    #[test]
    fn pages_of_comments_read_as_one_list_with_bots_and_ghosts_marked() {
        let comments = parse_comments(PAGES.as_bytes()).unwrap();

        let [yes, bot, ghost] = comments.as_slice() else {
            panic!("three comments: {comments:?}");
        };
        assert_eq!(
            yes,
            &ThreadComment {
                id: 1,
                thread: 71,
                author: "octocat".into(),
                bot: false,
                body: "yes".into(),
                created: Timestamp(1_791_610_314),
            }
        );
        assert!(bot.bot && bot.thread == 7);
        assert!(ghost.bot && ghost.body.is_empty());
    }

    #[test]
    fn an_answer_that_is_not_comments_is_unreadable() {
        for text in ["{}", "", r#"[[{"id": 1}]]"#] {
            assert!(
                matches!(
                    parse_comments(text.as_bytes()),
                    Err(ForgeError::Unreadable(_))
                ),
                "{text}"
            );
        }
    }

    #[test]
    fn only_admin_and_write_may_answer() {
        let said = |permission| format!(r#"{{"permission":"{permission}","role_name":"x"}}"#);
        for (permission, may) in [
            ("admin", true),
            ("write", true),
            ("read", false),
            ("none", false),
        ] {
            let text = said(permission);
            assert_eq!(parse_permission(text.as_bytes()), Ok(may), "{text}");
        }
        assert!(matches!(
            parse_permission(said("owner").as_bytes()),
            Err(ForgeError::Unreadable(_))
        ));
    }
}
