//! Whether kelpie's GitHub App covers a project's repo: an App for its
//! owner, installed on it, that mints a token

use super::Line;
use crate::github::{GithubApp, TokenError};
use crate::ports::Forge;
use crate::settings::{ForgeSlug, Settings};

/// The line for who a ruling mentions on `repo`, when it needs one: the App
/// covers the repo, `git.maintainer` is unset, and an organization owns the
/// repo, so a ruling mentions no one and notifies no one
pub(super) fn maintainer(
    subject: String,
    repo: &ForgeSlug,
    settings: &Settings,
    forge: &dyn Forge,
    github: &dyn GithubApp,
) -> Option<Line> {
    // Held in the token's cache, so this asks GitHub nothing more than the App's line did.
    if settings.git.maintainer.is_some() || github.token(repo).is_err() {
        return None;
    }
    let owner = repo.owner();
    match forge.owner_is_user(repo) {
        Ok(true) => None,
        Ok(false) => Some(Line::missing(
            subject,
            format!("{owner} is an organization, so a ruling mentions no one and notifies no one"),
            "set `git.maintainer` to your GitHub login in the project's `[app.dogs.kelpie.git]` table",
        )),
        Err(e) => Some(Line::unsure(
            subject,
            format!("cannot tell whether {owner} is a user: {e}"),
            "run doctor again, or set `git.maintainer` to your GitHub login",
        )),
    }
}

/// The line for `repo`'s App, under `subject`
pub(super) fn app(subject: String, repo: &ForgeSlug, github: &dyn GithubApp) -> Line {
    let slug = repo.as_str();
    match github.token(repo) {
        Ok(_) => Line::ok(
            subject,
            format!("kelpie's App is installed on {slug} and mints a token"),
        ),
        // No App is unsure, not missing: the App is optional, and without one kelpie
        // falls back to posting from the user's own `gh` login.
        Err(TokenError::NoApp(owner)) => Line::unsure(
            subject,
            format!("kelpie has no GitHub App for {owner}"),
            format!(
                "run `shep kelpie github setup`, with `--org {owner}` when {owner} is an \
                 organization, then install the App on {slug} from the page it opens; until \
                 then kelpie posts as your own login, which GitHub never notifies you of"
            ),
        ),
        Err(TokenError::NotInstalled { app, .. }) => Line::missing(
            subject,
            format!("{} is not installed on {slug}", app.slug),
            format!("install it there from {}", app.install_url()),
        ),
        Err(TokenError::Api(e)) if e.passes() => {
            Line::unsure(subject, e.to_string(), "run doctor again")
        }
        Err(e) => Line::missing(
            subject,
            format!("no token mints: {e}"),
            "put right what it names, or register the App again with `shep kelpie github setup --replace`",
        ),
    }
}
