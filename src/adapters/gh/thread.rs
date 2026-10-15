//! The comments written on a repo's threads, and who may answer a ruling in them

use serde::Deserialize;

use super::conditional::{Etags, GhApi};
use super::{gh, gh_as, unreadable};
use crate::ports::{CommentMarks, ForgeError, ThreadComment, Thumb, Timestamp};
use crate::settings::ForgeSlug;

// One page is as many comments as GitHub gives.
const PER_PAGE: &str = "per_page=100";

pub(super) fn comments_since(
    etags: &Etags,
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
    let pages = etags.pages(&GhApi, None, &path)?;
    parse_comments(format!("[{}]", join(&pages)).as_bytes())
}

pub(super) fn issue_comments(
    etags: &Etags,
    repo: &ForgeSlug,
    number: u64,
) -> Result<Vec<ThreadComment>, ForgeError> {
    let path = format!(
        "repos/{}/issues/{number}/comments?{PER_PAGE}",
        repo.as_str()
    );
    let pages = etags.pages(&GhApi, None, &path)?;
    parse_comments(format!("[{}]", join(&pages)).as_bytes())
}

// The first 100 review threads of a pull request with the first 100 comments
// of each, as the conversation read for a worker is (`talk`). A pull request
// past those is not expected; GraphQL has no `since` for a thread's
// comments, so the ones before it are dropped here.
const REPLIES: &str = "query($owner: String!, $name: String!, $number: Int!) { \
    repository(owner: $owner, name: $name) { pullRequest(number: $number) { \
    reviewThreads(first: 100) { nodes { comments(first: 100) { nodes { \
    databaseId body createdAt author { login __typename } } } } } } } }";

pub(super) fn thread_replies_since(
    repo: &ForgeSlug,
    number: u64,
    since: Timestamp,
) -> Result<Vec<ThreadComment>, ForgeError> {
    let (owner, name) = repo.as_str().split_once('/').unwrap_or_default();
    let stdout = gh_as(
        None,
        &[
            "api",
            "graphql",
            "-f",
            &format!("query={REPLIES}"),
            "-f",
            &format!("owner={owner}"),
            "-f",
            &format!("name={name}"),
            "-F",
            &format!("number={number}"),
        ],
    )?;
    parse_thread_replies(&stdout, number, since)
}

// GraphQL's answer to [`REPLIES`], the comments written at `since` or later,
// oldest first. A deleted account is a ghost, and anything but a user is a bot.
fn parse_thread_replies(
    stdout: &[u8],
    number: u64,
    since: Timestamp,
) -> Result<Vec<ThreadComment>, ForgeError> {
    #[derive(Deserialize)]
    struct Reply {
        data: Data,
    }
    #[derive(Deserialize)]
    struct Data {
        repository: Repository,
    }
    #[derive(Deserialize)]
    struct Repository {
        #[serde(rename = "pullRequest")]
        pull_request: Pr,
    }
    #[derive(Deserialize)]
    struct Pr {
        #[serde(rename = "reviewThreads")]
        threads: Nodes<Thread>,
    }
    #[derive(Deserialize)]
    struct Nodes<T> {
        nodes: Vec<T>,
    }
    #[derive(Deserialize)]
    struct Thread {
        comments: Nodes<Comment>,
    }
    #[derive(Deserialize)]
    struct Comment {
        #[serde(rename = "databaseId")]
        id: u64,
        body: Option<String>,
        #[serde(rename = "createdAt")]
        created: String,
        author: Option<Author>,
    }
    #[derive(Deserialize)]
    struct Author {
        login: String,
        #[serde(rename = "__typename")]
        kind: String,
    }
    let reply: Reply = serde_json::from_slice(stdout).map_err(|_| unreadable(stdout))?;
    let mut replies = Vec::new();
    for comment in (reply.data.repository.pull_request.threads.nodes.into_iter())
        .flat_map(|thread| thread.comments.nodes)
    {
        let created = comment.created.parse::<jiff::Timestamp>().ok();
        let created = created.and_then(|at| u64::try_from(at.as_second()).ok());
        let Some(created) = created.map(Timestamp) else {
            return Err(unreadable(stdout));
        };
        if created < since {
            continue;
        }
        let (author, bot) = match comment.author {
            Some(author) => (author.login, author.kind != "User"),
            None => ("ghost".to_owned(), true),
        };
        replies.push(ThreadComment {
            id: comment.id,
            thread: number,
            author,
            bot,
            body: comment.body.unwrap_or_default(),
            created,
        });
    }
    replies.sort_by_key(|c| (c.created, c.id));
    Ok(replies)
}

// The pages as the elements of one list
fn join(pages: &[Vec<u8>]) -> String {
    let texts: Vec<_> = pages.iter().map(|p| String::from_utf8_lossy(p)).collect();
    texts.join(",")
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

// A comment's author and words, its reactions and who made each of its edits.
// Edits are only on GraphQL, and GraphQL finds a comment by its node id.
const MARKS: &str = "query($id: ID!, $after: String) { node(id: $id) { ... on IssueComment { \
    author { login } body \
    reactions(first: 100) { nodes { content user { login } } } \
    userContentEdits(first: 100, after: $after) { \
    pageInfo { hasNextPage endCursor } nodes { editor { login } } } } } }";

pub(super) fn comment_marks(
    etags: &Etags,
    repo: &ForgeSlug,
    comment: u64,
) -> Result<CommentMarks, ForgeError> {
    let path = format!("repos/{}/issues/comments/{comment}", repo.as_str());
    let node = parse_node_id(&etags.get(&GhApi, None, &path)?)?;
    let query = format!("query={MARKS}");
    let id = format!("id={node}");
    let (mut marks, mut next) = parse_marks(&gh(&["api", "graphql", "-f", &query, "-f", &id])?)?;
    // A tick is trusted only if every editor is known, so every page of
    // edits is read; the other pages' reactions and words are the first's.
    while let Some(cursor) = next {
        let after = format!("after={cursor}");
        let page = gh(&["api", "graphql", "-f", &query, "-f", &id, "-f", &after])?;
        let (more, cursor) = parse_marks(&page)?;
        marks.editors.extend(more.editors);
        next = cursor;
    }
    Ok(marks)
}

fn parse_node_id(stdout: &[u8]) -> Result<String, ForgeError> {
    #[derive(Deserialize)]
    struct Comment {
        node_id: String,
    }
    let comment: Comment = serde_json::from_slice(stdout).map_err(|_| unreadable(stdout))?;
    Ok(comment.node_id)
}

// GraphQL's answer to [`MARKS`]. A deleted account is a ghost, which is no
// one's login. Reactions other than thumbs are left out.
fn parse_marks(stdout: &[u8]) -> Result<(CommentMarks, Option<String>), ForgeError> {
    #[derive(Deserialize)]
    struct Reply {
        data: Data,
    }
    #[derive(Deserialize)]
    struct Data {
        node: Option<Comment>,
    }
    #[derive(Deserialize)]
    struct Comment {
        author: Option<Login>,
        body: String,
        reactions: Nodes<Reaction>,
        #[serde(rename = "userContentEdits")]
        edits: Edits,
    }
    #[derive(Deserialize)]
    struct Nodes<T> {
        nodes: Option<Vec<T>>,
    }
    #[derive(Deserialize)]
    struct Edits {
        nodes: Option<Vec<Edit>>,
        #[serde(rename = "pageInfo", default)]
        page: Option<Page>,
    }
    #[derive(Deserialize)]
    struct Page {
        #[serde(rename = "hasNextPage")]
        more: bool,
        #[serde(rename = "endCursor")]
        cursor: Option<String>,
    }
    #[derive(Deserialize)]
    struct Reaction {
        content: String,
        user: Option<Login>,
    }
    #[derive(Deserialize)]
    struct Edit {
        editor: Option<Login>,
    }
    #[derive(Deserialize)]
    struct Login {
        login: String,
    }
    let ghost = || "ghost".to_owned();
    let reply: Reply = serde_json::from_slice(stdout).map_err(|_| unreadable(stdout))?;
    let comment = reply.data.node.ok_or_else(|| unreadable(stdout))?;
    let thumbs = (comment.reactions.nodes.into_iter().flatten())
        .filter_map(|reaction| {
            let up = match reaction.content.as_str() {
                "THUMBS_UP" => true,
                "THUMBS_DOWN" => false,
                _ => return None,
            };
            let login = reaction.user.map_or_else(ghost, |user| user.login);
            Some(Thumb { login, up })
        })
        .collect();
    let editors = (comment.edits.nodes.into_iter().flatten())
        .map(|edit| edit.editor.map_or_else(ghost, |editor| editor.login))
        .collect();
    // More edits than a page hold, but no cursor to the rest: not all known.
    let next = match comment.edits.page {
        Some(Page { more: true, cursor }) => Some(cursor.ok_or_else(|| unreadable(stdout))?),
        _ => None,
    };
    let marks = CommentMarks {
        author: comment.author.map_or_else(ghost, |author| author.login),
        body: comment.body,
        thumbs,
        editors,
    };
    Ok((marks, next))
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

    // GraphQL's answer to `REPLIES` for a pull request with two review
    // threads: the contributor replying to a bot, the maintainer answering,
    // a comment from before the read, and a deleted account. The fixture is
    // written from GitHub's schema, not recorded: a worker cannot run
    // `gh api`.
    const REPLIED: &str = include_str!("../../../fixtures/gh-graphql-thread-replies.json");

    #[test]
    fn the_gh_adapter_parses_the_recorded_thread_replies() {
        let since = Timestamp(1_791_610_000);

        let replies = parse_thread_replies(REPLIED.as_bytes(), 41, since).unwrap();

        let seen: Vec<_> = replies
            .iter()
            .map(|c| (c.id, c.author.as_str(), c.bot, c.thread, c.created.0))
            .collect();
        assert_eq!(
            seen,
            [
                (3_001, "newcomer", false, 41, 1_791_610_314),
                (3_002, "Boss", false, 41, 1_791_610_400),
                (3_003, "coderabbitai", true, 41, 1_791_610_500),
                (3_004, "ghost", true, 41, 1_791_610_600),
            ]
        );
        assert!(replies[0].body.starts_with("Fixed in"));
        // The comment before `since` is left out
        let all = parse_thread_replies(REPLIED.as_bytes(), 41, Timestamp(0)).unwrap();
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn a_thread_reply_answer_that_is_not_threads_is_unreadable() {
        for text in ["{}", "", r#"{"data":{"repository":{"pullRequest":null}}}"#] {
            assert!(
                matches!(
                    parse_thread_replies(text.as_bytes(), 1, Timestamp(0)),
                    Err(ForgeError::Unreadable(_))
                ),
                "{text}"
            );
        }
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

    // GraphQL's documented answer to `MARKS` for a ruling's comment the
    // maintainer ticked a box in and gave a thumbs up, with a laugh from
    // someone else and one reaction by a deleted account. Documented, not
    // recorded, for the reason `PAGES` is.
    const MARKED: &str = r#"{"data":{"node":{
      "author":{"login":"kelpie-o"},
      "body":"- [x] A",
      "reactions":{"nodes":[
        {"content":"THUMBS_UP","user":{"login":"octocat"}},
        {"content":"LAUGH","user":{"login":"stranger"}},
        {"content":"THUMBS_DOWN","user":null}]},
      "userContentEdits":{"nodes":[{"editor":{"login":"octocat"}}]}}}}"#;

    #[test]
    fn a_comments_thumbs_and_editors_are_read_and_other_reactions_left_out() {
        let (marks, next) = parse_marks(MARKED.as_bytes()).unwrap();

        assert_eq!(next, None);

        assert_eq!(
            marks,
            CommentMarks {
                author: "kelpie-o".into(),
                body: "- [x] A".into(),
                thumbs: vec![
                    Thumb {
                        login: "octocat".into(),
                        up: true
                    },
                    Thumb {
                        login: "ghost".into(),
                        up: false
                    },
                ],
                editors: vec!["octocat".into()],
            }
        );
    }

    #[test]
    fn edits_past_the_first_page_are_asked_for_by_their_cursor() {
        let page = |info: &str| {
            format!(
                r#"{{"data":{{"node":{{"author":null,"body":"","reactions":{{"nodes":[]}},
                "userContentEdits":{{"pageInfo":{info},
                "nodes":[{{"editor":{{"login":"octocat"}}}}]}}}}}}}}"#
            )
        };

        let more = page(r#"{"hasNextPage":true,"endCursor":"Y3Vyc29y"}"#);
        let last = page(r#"{"hasNextPage":false,"endCursor":"Y3Vyc29y"}"#);
        let lost = page(r#"{"hasNextPage":true,"endCursor":null}"#);

        assert_eq!(
            parse_marks(more.as_bytes()).unwrap().1.as_deref(),
            Some("Y3Vyc29y")
        );
        assert_eq!(parse_marks(last.as_bytes()).unwrap().1, None);
        // Trusting a tick on a history not all read is the harm, so it fails.
        assert!(matches!(
            parse_marks(lost.as_bytes()),
            Err(ForgeError::Unreadable(_))
        ));
    }

    #[test]
    fn a_comment_never_edited_has_no_editors() {
        let text = r#"{"data":{"node":{"author":null,"body":"","reactions":{"nodes":[]},
                        "userContentEdits":{"nodes":null}}}}"#;

        let (marks, _) = parse_marks(text.as_bytes()).unwrap();

        assert_eq!(marks.editors, Vec::<String>::new());
        assert_eq!(marks.author, "ghost");
    }

    #[test]
    fn a_comment_the_forge_no_longer_has_is_unreadable() {
        for text in [r#"{"data":{"node":null}}"#, "{}", ""] {
            assert!(
                matches!(parse_marks(text.as_bytes()), Err(ForgeError::Unreadable(_))),
                "{text}"
            );
        }
    }

    #[test]
    fn a_comments_node_id_is_read() {
        let text = r#"{"id": 7, "node_id": "IC_kwDOA", "body": "x"}"#;

        assert_eq!(parse_node_id(text.as_bytes()).as_deref(), Ok("IC_kwDOA"));
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
