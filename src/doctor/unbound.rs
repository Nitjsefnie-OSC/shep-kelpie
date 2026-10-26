//! Each agent file that binds nothing, which no project can name until it does

use std::path::Path;

use super::Line;
use crate::agents::Agents;
use crate::agents::legacy;

/// A line for each file in `agents` that binds no harness, model or effort,
/// then one for each file in `folder` the migration would skip
///
/// A stub the migration would write names it as the fix: it inherited a
/// binding that no baseline ships now. Any other file names the command that
/// binds it. A skipped file sets a `harness` the loader refuses, so only
/// `legacy::pending` sees it.
pub(super) fn checks(agents: &Agents, folder: &Path) -> Vec<Line> {
    let pending = legacy::pending(folder).unwrap_or_default();
    let unbound = agents.unbound().iter().map(|unbound| {
        let file = format!("{}.md", unbound.name);
        let migratable = pending.wrote.iter().any(|wrote| wrote.file == file);
        Line::missing(
            format!("agent {}", unbound.name),
            format!("`{file}` binds no harness, model or effort"),
            if migratable {
                "shep kelpie upgrade, or shep kelpie agents migrate".to_owned()
            } else {
                unbound.command()
            },
        )
    });
    let skipped = pending.skipped.iter().map(|skipped| {
        Line::missing(
            format!("agent {}", skipped.file.trim_end_matches(".md")),
            format!(
                "`{}` is skipped by the migration: {}",
                skipped.file, skipped.why
            ),
            format!(
                "write its `model` and `effort`, or take its `harness` out of `{}`",
                skipped.file
            ),
        )
    });
    unbound.chain(skipped).collect()
}
