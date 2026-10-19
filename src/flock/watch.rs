//! `shep kelpie watch [<issue> | pm] [--back <n>] [--full]`: a project's
//! runner steps and session transcripts, followed live
//!
//! It reads the runner's `steps.jsonl` and the transcripts it names from
//! disk, so it needs the shepherd only to find the project from the folder
//! it runs in.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::home::ProjectName;
use crate::ticket::TicketId;
use crate::watch::{Options, Scope, Watch};

/// How often the log and transcripts are read again
const EVERY: Duration = Duration::from_millis(250);

/// What `watch` was asked for
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Ask {
    pub scope: Scope,
    pub back: usize,
    pub full: bool,
}

/// Reads `watch`'s arguments
///
/// # Errors
///
/// [`super::USAGE`] when they are not `watch`'s.
pub(super) fn read_args(args: &[&str]) -> Result<Ask, String> {
    let usage = || super::USAGE.to_owned();
    let mut ask = Ask {
        scope: Scope::Project,
        back: 0,
        full: false,
    };
    let mut scoped = false;
    let mut args = args.iter().copied();
    while let Some(arg) = args.next() {
        let number = |text: &str| text.parse::<usize>().map_err(|_| usage());
        match arg {
            "--full" => ask.full = true,
            "--back" => ask.back = number(args.next().ok_or_else(usage)?)?,
            "pm" if !scoped => {
                ask.scope = Scope::Pm;
                scoped = true;
            }
            word if !scoped && is_ticket(word) => match word.parse::<TicketId>() {
                Ok(ticket) => {
                    ask.scope = Scope::Issue(ticket);
                    scoped = true;
                }
                Err(why) => return Err(format!("{word:?} is not a ticket id: {why}")),
            },
            other => match other.strip_prefix("--back=") {
                Some(n) => ask.back = number(n)?,
                None => return Err(usage()),
            },
        }
    }
    Ok(ask)
}

// `k12` or `12`: the shape of a ticket id, so a mistyped flag is still a
// usage error rather than a ticket id that reads as one
fn is_ticket(word: &str) -> bool {
    let digits = word.strip_prefix('k').unwrap_or(word);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

/// The folder of `project`, which holds its `steps.jsonl` once its runner
/// logs a step
///
/// The file itself may not exist yet: a runner with nothing to do logs no
/// step, and `watch` waits for the first.
///
/// # Errors
///
/// A message when the project has no folder, which its runner makes as it opens.
fn folder_of(kelpie_home: &Path, project: &ProjectName) -> Result<PathBuf, String> {
    let folder = kelpie_home.join(project.as_str());
    if folder.is_dir() {
        return Ok(folder);
    }
    Err(format!(
        "{project} has no folder at {}: `shep kelpie start` runs it",
        folder.display()
    ))
}

/// Follows `project` until interrupted, printing what the stream shows
///
/// # Errors
///
/// A message when the project has no folder yet, or the output cannot be written.
pub(super) fn follow(kelpie_home: &Path, project: &ProjectName, ask: Ask) -> Result<(), String> {
    let folder = folder_of(kelpie_home, project)?;
    let options = Options {
        scope: ask.scope,
        back: ask.back,
        full: ask.full,
        zone: jiff::tz::TimeZone::system(),
    };
    let (mut watch, first) = Watch::start(folder, options);
    let mut out = std::io::stdout().lock();
    let mut show = |lines: Vec<crate::watch::Line>| -> Result<(), String> {
        for line in lines {
            writeln!(out, "{line}").map_err(|e| format!("cannot write: {e}"))?;
        }
        out.flush().map_err(|e| format!("cannot write: {e}"))
    };
    show(first)?;
    loop {
        std::thread::sleep(EVERY);
        show(watch.poll())?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::ticket;

    fn ask(line: &str) -> Result<Ask, String> {
        let words: Vec<&str> = line.split_whitespace().collect();
        read_args(&words)
    }

    #[test]
    fn a_running_project_with_no_step_logged_yet_is_followed_not_refused() {
        let home = tempfile::tempdir().unwrap();
        let koji = ProjectName::try_from("koji").unwrap();
        assert!(folder_of(home.path(), &koji).is_err());
        std::fs::create_dir(home.path().join("koji")).unwrap();
        let folder = folder_of(home.path(), &koji).unwrap();
        let (mut watch, first) = Watch::start(
            folder.clone(),
            Options {
                scope: Scope::Project,
                back: 0,
                full: false,
                zone: jiff::tz::TimeZone::UTC,
            },
        );
        assert!(first.is_empty() && watch.poll().is_empty());
        std::fs::write(
            folder.join(crate::runner::STEPS_FILE),
            "{\"at\":\"2026-10-10T12:00:00.000Z\",\"step\":\"finished\",\"issue\":7}\n",
        )
        .unwrap();
        assert_eq!(watch.poll().len(), 1);
    }

    #[test]
    fn it_watches_the_whole_project_from_its_end_unless_told_otherwise() {
        assert_eq!(
            ask("").unwrap(),
            Ask {
                scope: Scope::Project,
                back: 0,
                full: false
            }
        );
    }

    #[test]
    fn it_takes_an_issue_or_pm_and_the_flags_in_any_order() {
        let issue = Ask {
            scope: Scope::Issue(ticket(7)),
            back: 20,
            full: true,
        };
        for line in [
            "7 --back 20 --full",
            "--full 7 --back=20",
            "--back 20 --full 7",
        ] {
            assert_eq!(ask(line).unwrap(), issue, "{line}");
        }
        assert_eq!(ask("pm --back 3").unwrap().scope, Scope::Pm);
    }

    #[test]
    fn anything_else_is_the_usage() {
        for line in [
            "7 8",
            "pm 7",
            "seven",
            "--back",
            "--back x",
            "--back=-1",
            "0",
            "--loud",
        ] {
            assert!(ask(line).is_err(), "{line}");
        }
    }
}
