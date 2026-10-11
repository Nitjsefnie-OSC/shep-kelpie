//! `shep kelpie watch [<issue> | pm] [--back <n>] [--full]`: a project's
//! runner steps and session transcripts, followed live
//!
//! It reads the runner's log and the transcripts it names from disk, so it
//! needs the shepherd only to find the project from the folder it runs in.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::runner::ProjectName;
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
            issue if !scoped && issue.bytes().all(|b| b.is_ascii_digit()) && !issue.is_empty() => {
                match issue.parse::<u64>() {
                    Ok(issue) if issue > 0 => ask.scope = Scope::Issue(issue),
                    _ => return Err(format!("{issue:?} is not an issue number")),
                }
                scoped = true;
            }
            other => match other.strip_prefix("--back=") {
                Some(n) => ask.back = number(n)?,
                None => return Err(usage()),
            },
        }
    }
    Ok(ask)
}

/// The runner's log under the shepherd's home, as shep keeps it
fn log_of(shep_home: &Path, project: &ProjectName) -> PathBuf {
    let name = format!("{}-0-out.log", project.as_str());
    shep_home.join("logs").join(name)
}

/// Follows `project` until interrupted, printing what the stream shows
///
/// # Errors
///
/// A message when the project has no log yet, or the output cannot be written.
pub(super) fn follow(shep_home: &Path, project: &ProjectName, ask: Ask) -> Result<(), String> {
    let log = log_of(shep_home, project);
    if !log.is_file() {
        return Err(format!(
            "{} has no log at {}: `shep kelpie start` runs it",
            project,
            log.display()
        ));
    }
    let options = Options {
        scope: ask.scope,
        back: ask.back,
        full: ask.full,
        zone: jiff::tz::TimeZone::system(),
    };
    let (mut watch, first) = Watch::start(log, options);
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

    fn ask(line: &str) -> Result<Ask, String> {
        let words: Vec<&str> = line.split_whitespace().collect();
        read_args(&words)
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
            scope: Scope::Issue(7),
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
