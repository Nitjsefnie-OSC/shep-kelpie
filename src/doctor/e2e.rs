//! Whether this machine can reach a project's end-to-end host
//!
//! It makes no connection to the host: it reads the table and looks for the
//! programs a run copies and runs with.

use super::Line;
use super::host::Host;
use crate::settings::Settings;

// What a run calls on this machine: `ssh` runs the command, `rsync` copies
const PROGRAMS: [&str; 2] = ["ssh", "rsync"];

/// The line for a project with an `e2e` table, or none without one
pub(super) fn line(subject: String, settings: &Settings, host: &dyn Host) -> Option<Line> {
    let e2e = settings.e2e.as_ref()?;
    let gone: Vec<String> = PROGRAMS
        .into_iter()
        .filter(|program| !host.on_path(program))
        .map(|program| format!("`{program}`"))
        .collect();
    if gone.is_empty() {
        return Some(Line::ok(
            subject,
            format!(
                "runs `{}` on {} over ssh and rsync",
                e2e.command.as_path().display(),
                e2e.host.as_str()
            ),
        ));
    }
    let are = if gone.len() == 1 { "is" } else { "are" };
    Some(Line::missing(
        subject,
        format!(
            "{} {are} not on `PATH`, which a run of `{}` on {} needs",
            gone.join(" and "),
            e2e.command.as_path().display(),
            e2e.host.as_str()
        ),
        format!(
            "install {}, or take the `[app.dogs.kelpie.e2e]` table out of the project's settings",
            gone.join(" and ")
        ),
    ))
}
