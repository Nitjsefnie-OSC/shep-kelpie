//! What the logged-in account may do on a repo, and whether a review bot works there

use serde::Deserialize;

use super::{gh, unreadable};
use crate::ports::{ForgeError, Timestamp};
use crate::review_bot::Login;
use crate::settings::ForgeSlug;

pub(super) fn can_push(repo: &ForgeSlug) -> Result<bool, ForgeError> {
    parse_permission(&gh(&[
        "repo",
        "view",
        repo.as_str(),
        "--json",
        "viewerPermission",
    ])?)
}

// GitHub's search finds a bot's reviews by its app name, which is the
// GraphQL login. The newest-updated pull request it reviewed stands for its
// last review: search carries no review's own time.
pub(super) fn last_bot_review(
    repo: &ForgeSlug,
    login: Login<'_>,
) -> Result<Option<Timestamp>, ForgeError> {
    let reviewer = format!("app/{}", login.graphql);
    parse_last_review(&gh(&[
        "search",
        "prs",
        "--repo",
        repo.as_str(),
        "--reviewed-by",
        &reviewer,
        "--sort",
        "updated",
        "--order",
        "desc",
        "--limit",
        "1",
        "--json",
        "number,updatedAt",
    ])?)
}

fn parse_permission(stdout: &[u8]) -> Result<bool, ForgeError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct View {
        viewer_permission: String,
    }
    let view: View = serde_json::from_slice(stdout).map_err(|_| unreadable(stdout))?;
    match view.viewer_permission.as_str() {
        "ADMIN" | "MAINTAIN" | "WRITE" => Ok(true),
        "TRIAGE" | "READ" => Ok(false),
        _ => Err(unreadable(stdout)),
    }
}

fn parse_last_review(stdout: &[u8]) -> Result<Option<Timestamp>, ForgeError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Found {
        updated_at: String,
    }
    let found: Vec<Found> = serde_json::from_slice(stdout).map_err(|_| unreadable(stdout))?;
    let Some(newest) = found.first() else {
        return Ok(None);
    };
    let at: jiff::Timestamp = newest.updated_at.parse().map_err(|_| unreadable(stdout))?;
    let seconds = u64::try_from(at.as_second()).map_err(|_| unreadable(stdout))?;
    Ok(Some(Timestamp(seconds)))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Recorded from gh 2.96: `gh repo view <repo> --json viewerPermission`
    // on this repo, as its maintainer, and on cli/cli, as a stranger.
    const ADMIN: &str = r#"{"viewerPermission":"ADMIN"}"#;
    const READ: &str = r#"{"viewerPermission":"READ"}"#;

    // Recorded from gh 2.96: `gh search prs --repo <repo> --reviewed-by
    // app/coderabbitai --sort updated --order desc --limit 1 --json
    // number,updatedAt` on shep-pm/shep, which CodeRabbit reviews, and on
    // cli/cli, which it does not.
    const REVIEWED: &str = r#"[{"number":918,"updatedAt":"2026-10-16T02:42:33Z"}]"#;
    const UNREVIEWED: &str = "[]";

    #[test]
    fn only_a_writer_may_push() {
        assert_eq!(parse_permission(ADMIN.as_bytes()), Ok(true));
        assert_eq!(parse_permission(READ.as_bytes()), Ok(false));
        for (permission, pushes) in [("MAINTAIN", true), ("WRITE", true), ("TRIAGE", false)] {
            let text = format!(r#"{{"viewerPermission":"{permission}"}}"#);
            assert_eq!(parse_permission(text.as_bytes()), Ok(pushes), "{text}");
        }
    }

    #[test]
    fn a_permission_it_does_not_know_is_unreadable() {
        for text in [r#"{"viewerPermission":"OWNER"}"#, "{}", ""] {
            assert!(
                matches!(
                    parse_permission(text.as_bytes()),
                    Err(ForgeError::Unreadable(_))
                ),
                "{text}"
            );
        }
    }

    #[test]
    fn a_bots_last_review_is_the_update_of_the_pull_request_the_search_finds() {
        assert_eq!(
            parse_last_review(REVIEWED.as_bytes()),
            Ok(Some(Timestamp(1_792_118_553)))
        );
        assert_eq!(parse_last_review(UNREVIEWED.as_bytes()), Ok(None));
        for text in ["{}", r#"[{"number":918}]"#, r#"[{"updatedAt":"soon"}]"#] {
            assert!(
                matches!(
                    parse_last_review(text.as_bytes()),
                    Err(ForgeError::Unreadable(_))
                ),
                "{text}"
            );
        }
    }
}
