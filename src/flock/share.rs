//! One runner per checkout and per repo
//!
//! `add` and `set` check it before a table is written, and a runner checks it
//! as it starts and when it rereads a moved `git.checkout` or `git.remote`,
//! since a table can be written another way.

use std::collections::BTreeMap;
use std::path::Path;

use serde_json::{Map, Value};
use shep_client::shep_core::protocol::request::ProcessInfo;

use crate::settings::Settings;
use crate::shepherd::Connection;

/// Another project already runs the checkout or repo
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Taken {
    /// The project that runs it
    pub(crate) sheep: String,
    slug: String,
}

impl Taken {
    /// What is taken and by whom, with no word of what to do about it
    pub(crate) fn clash(&self) -> String {
        let Self { sheep, slug } = self;
        format!("project `{sheep}` already runs this checkout or {slug}")
    }
}

impl core::fmt::Display for Taken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}: `shep kelpie start {}`", self.clash(), self.sheep)
    }
}

/// Refuses a checkout or repo that another runner already runs
///
/// One runner per checkout and per repo: two would take the same issues. It
/// reads each table by its raw keys, so one that no longer parses still counts.
/// `name` is the project asking, whose own row and table are left out.
///
/// # Errors
///
/// [`Taken`] naming the first other project that runs `root` or `slug`.
pub(crate) fn share_none(
    rows: &[ProcessInfo],
    tables: &BTreeMap<String, Map<String, Value>>,
    name: &str,
    home: &Path,
    root: &Path,
    slug: &str,
) -> Result<(), Taken> {
    for row in rows.iter().filter(|r| r.name != name) {
        let sheep = row.name.as_str();
        let Some(other) = tables.get(sheep) else {
            continue;
        };
        // Today's `git` table, or the top-level key a table from before it holds.
        let text = |key: &str, old: &str| {
            let git = other.get("git").and_then(Value::as_object);
            let set = git.and_then(|git| git.get(key)).or_else(|| other.get(old));
            set.and_then(Value::as_str).map(str::to_owned)
        };
        let runs_from = text("checkout", "repo").map(|repo| match repo.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => repo.into(),
        });
        // With no `git.remote`, its runner reads the repo from its checkout's `origin`.
        let same_repo = match text("remote", "forge") {
            Some(remote) => remote == slug,
            None => (runs_from.as_deref())
                .and_then(|folder| super::Checkout::of(folder).ok())
                .is_some_and(|theirs| theirs.forge.as_str() == slug),
        };
        if runs_from.as_deref() == Some(root) || same_repo {
            return Err(Taken {
                sheep: sheep.to_owned(),
                slug: slug.to_owned(),
            });
        }
    }
    Ok(())
}

/// Whether another project runs the checkout or repo `settings` name, asking
/// the shepherd for the flock and its tables
///
/// Settings whose checkout is no git checkout are not taken by anyone: the
/// start's own checks refuse them.
///
/// # Errors
///
/// A message when the shepherd cannot be asked for the flock or its tables.
pub(crate) async fn check(
    client: &dyn Connection,
    name: &str,
    home: &Path,
    settings: &Settings,
) -> Result<Option<Taken>, String> {
    let Ok(remote) = crate::runner::check_checkout(settings) else {
        return Ok(None);
    };
    let (rows, others) = (super::flock(client).await?, super::tables(client).await?);
    let root = &settings.git.checkout;
    Ok(share_none(&rows, &others, name, home, root, remote.as_str()).err())
}
