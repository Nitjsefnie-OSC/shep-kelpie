//! Each agent file that binds nothing, which no project can name until it does

use std::path::Path;

use super::Line;
use crate::agents::Agents;
use crate::agents::legacy;

/// A line for each file the migration would write, then each other file in `agents` that binds
/// no harness, model or effort, then one for each file in `folder` the migration would skip
///
/// A stub the migration would write names it as the fix: it inherited a binding that no
/// baseline ships now. It is read from `legacy::pending`, which sees the folder when `agents` is
/// the embedded book that stands in for a folder that cannot be read. Any other unbound file
/// names the command that binds it.
pub(super) fn checks(agents: &Agents, folder: &Path) -> Vec<Line> {
    let pending = legacy::pending(folder).unwrap_or_default();
    let migratable = pending
        .wrote
        .iter()
        .filter(|wrote| wrote.was_unbound())
        .map(|wrote| {
            let name = wrote.file.trim_end_matches(".md");
            unbound_line(
                name,
                &wrote.file,
                "shep kelpie upgrade, or shep kelpie agents migrate",
            )
        });
    let unbound = agents.unbound().iter().filter_map(|unbound| {
        let file = format!("{}.md", unbound.name);
        let listed = pending.wrote.iter().any(|wrote| wrote.file == file);
        (!listed).then(|| unbound_line(unbound.name.as_str(), &file, &unbound.command()))
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
    migratable.chain(unbound).chain(skipped).collect()
}

fn unbound_line(name: &str, file: &str, fix: &str) -> Line {
    Line::missing(
        format!("agent {name}"),
        format!("`{file}` binds no harness, model or effort"),
        fix.to_owned(),
    )
}
