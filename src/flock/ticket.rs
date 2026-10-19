//! `shep kelpie ticket new|list|show|edit|close`: a local project's tickets, filed
//! and read as files in its `tickets/` folder (ADR 0014)
//!
//! The verbs work the files directly, so a ticket filed here is on the board at
//! the runner's next read, with the runner stopped or running.

use std::path::Path;

use crate::local_tickets::{Links, LocalError, LocalTickets, Standing};
use crate::ticket::TicketId;

/// What the verbs take, as their help says
pub const USAGE: &str = "\
usage: shep kelpie ticket new \"<title>\" [--body <text>] [--label <label>]...
                            [--parent <id>] [--blocked-by <id>]...
                                         files a ticket and prints its id
       shep kelpie ticket list [--all]   the open tickets, or every one, a line each
       shep kelpie ticket show <id>      a ticket's file
       shep kelpie ticket edit <id> [--title <text>] [--body <text>]
                            [--label <label>]... [--unlabel <label>]...
                            [--parent <id> | --no-parent]
                            [--blocked-by <id>]... [--unblock <id>]...
       shep kelpie ticket close <id> [--not-planned | --duplicate]";

/// Runs `ticket <args>` on the store in `folder`, and returns the lines to print
///
/// # Errors
///
/// A message when the verb or a flag is not one of [`USAGE`], an id is not a
/// ticket of the store, or the store cannot be read or written.
pub fn run(folder: &Path, args: &[&str]) -> Result<Vec<String>, String> {
    let store = LocalTickets::new(folder.to_owned());
    let refused = |e: LocalError| e.to_string();
    match args {
        ["new", rest @ ..] => {
            let flags = Flags::of(rest, &["--body", "--label", "--parent", "--blocked-by"], &[])?;
            let [title] = flags.words.as_slice() else {
                return Err(format!("`ticket new` takes one title\n\n{USAGE}"));
            };
            let links = Links {
                parent: flags.one("--parent")?.map(id).transpose()?,
                blocked_by: ids(&flags, "--blocked-by")?,
            };
            let labels = flags.all("--label");
            let body = flags.one("--body")?.unwrap_or_default();
            let made = store.create(title, body, &labels, links).map_err(refused)?;
            Ok(vec![made.to_string()])
        }
        ["list", rest @ ..] => {
            let flags = Flags::of(rest, &[], &["--all"])?;
            no_words(&flags)?;
            let all = flags.has("--all");
            let tickets = store.all().map_err(refused)?;
            Ok(tickets
                .into_iter()
                .filter(|(_, s)| all || s.standing.is_open())
                .map(|(ticket, s)| {
                    let labels = match s.labels.is_empty() {
                        true => String::new(),
                        false => format!(" [{}]", s.labels.join(", ")),
                    };
                    format!("{ticket}  {}  {}{labels}", s.standing.as_str(), s.title)
                })
                .collect())
        }
        ["show", ticket] => {
            let shown = store.get(id(ticket)?).map_err(refused)?;
            let text = shown.render().map_err(|e| e.to_string())?;
            Ok(text.lines().map(str::to_owned).collect())
        }
        ["edit", ticket, rest @ ..] => {
            let ticket = id(ticket)?;
            let valued = [
                "--title",
                "--body",
                "--label",
                "--unlabel",
                "--parent",
                "--blocked-by",
                "--unblock",
            ];
            let flags = Flags::of(rest, &valued, &["--no-parent"])?;
            no_words(&flags)?;
            if flags.has("--no-parent") && flags.one("--parent")?.is_some() {
                return Err("`--parent` and `--no-parent` contradict each other".to_owned());
            }
            let parent = flags.one("--parent")?.map(id).transpose()?;
            let (title, body) = (flags.one("--title")?, flags.one("--body")?);
            let (on, off) = (flags.all("--label"), flags.all("--unlabel"));
            let (block, unblock) = (ids(&flags, "--blocked-by")?, ids(&flags, "--unblock")?);
            store
                .update(ticket, |s| {
                    if let Some(title) = title {
                        s.title = title.to_owned();
                    }
                    if let Some(body) = body {
                        s.body = body.to_owned();
                    }
                    s.labels.retain(|l| !off.contains(&l.as_str()));
                    for label in on {
                        if !s.labels.iter().any(|l| l == label) {
                            s.labels.push(label.to_owned());
                        }
                    }
                    if flags.has("--no-parent") {
                        s.parent = None;
                    }
                    s.parent = parent.or(s.parent);
                    s.blocked_by.retain(|b| !unblock.contains(b));
                    for blocker in block {
                        if !s.blocked_by.contains(&blocker) {
                            s.blocked_by.push(blocker);
                        }
                    }
                })
                .map_err(refused)?;
            Ok(vec![ticket.to_string()])
        }
        ["close", ticket, rest @ ..] => {
            let ticket = id(ticket)?;
            let flags = Flags::of(rest, &[], &["--not-planned", "--duplicate"])?;
            no_words(&flags)?;
            let standing = match (flags.has("--not-planned"), flags.has("--duplicate")) {
                (false, false) => Standing::Closed,
                (true, false) => Standing::NotPlanned,
                (false, true) => Standing::Duplicate,
                (true, true) => {
                    return Err("`--not-planned` and `--duplicate` contradict each other".to_owned());
                }
            };
            store.update(ticket, |s| s.standing = standing).map_err(refused)?;
            Ok(vec![ticket.to_string()])
        }
        _ => Err(USAGE.to_owned()),
    }
}

// A local ticket's id, which `shep kelpie ticket` is the only way to name a GitHub one by
fn id(text: &str) -> Result<TicketId, String> {
    match text.parse::<TicketId>() {
        Ok(ticket @ TicketId::Local(_)) => Ok(ticket),
        Ok(TicketId::Github(_)) => Err(format!(
            "{text:?} is a GitHub issue, and `ticket` works the local store: its ids start with `k`"
        )),
        Err(e) => Err(format!("{text:?} is not a ticket id: {e}")),
    }
}

fn ids(flags: &Flags<'_>, flag: &str) -> Result<Vec<TicketId>, String> {
    flags.all(flag).into_iter().map(id).collect()
}

fn no_words(flags: &Flags<'_>) -> Result<(), String> {
    match flags.words.first() {
        None => Ok(()),
        Some(word) => Err(format!("{word:?} is not an argument of this verb\n\n{USAGE}")),
    }
}

// A verb's arguments, split into its words, the flags that take a value and the
// flags that take none
struct Flags<'a> {
    words: Vec<&'a str>,
    valued: Vec<(&'a str, &'a str)>,
    switches: Vec<&'a str>,
}

impl<'a> Flags<'a> {
    fn of(args: &[&'a str], valued: &[&str], switches: &[&str]) -> Result<Self, String> {
        let mut flags = Self {
            words: Vec::new(),
            valued: Vec::new(),
            switches: Vec::new(),
        };
        let mut args = args.iter().copied();
        while let Some(arg) = args.next() {
            if valued.contains(&arg) {
                let value = args
                    .next()
                    .ok_or_else(|| format!("`{arg}` takes a value\n\n{USAGE}"))?;
                flags.valued.push((arg, value));
            } else if switches.contains(&arg) {
                flags.switches.push(arg);
            } else if arg.starts_with("--") {
                return Err(format!("{arg:?} is not a flag of this verb\n\n{USAGE}"));
            } else {
                flags.words.push(arg);
            }
        }
        Ok(flags)
    }

    // The value of a flag given once, None when it is not given
    fn one(&self, flag: &str) -> Result<Option<&'a str>, String> {
        match self.all(flag).as_slice() {
            [] => Ok(None),
            [value] => Ok(Some(value)),
            _ => Err(format!("`{flag}` is given once")),
        }
    }

    // Every value of a flag, in the order given
    fn all(&self, flag: &str) -> Vec<&'a str> {
        (self.valued.iter())
            .filter(|(name, _)| *name == flag)
            .map(|(_, value)| *value)
            .collect()
    }

    fn has(&self, flag: &str) -> bool {
        self.switches.contains(&flag)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::short_tempdir;

    fn ticket(folder: &Path, args: &[&str]) -> Result<Vec<String>, String> {
        run(folder, args)
    }

    #[test]
    fn new_files_a_ticket_and_prints_its_id() {
        let home = short_tempdir();
        let folder = home.path().join("tickets");
        let args = ["new", "Add a thing", "--body", "Do it.", "--label", "ready-for-agent"];
        assert_eq!(ticket(&folder, &args).unwrap(), ["k1"]);
        assert_eq!(ticket(&folder, &["new", "Another"]).unwrap(), ["k2"]);
        let shown = ticket(&folder, &["show", "k1"]).unwrap().join("\n");
        assert!(shown.contains("title: \"Add a thing\""), "{shown}");
        assert!(shown.contains("labels: [\"ready-for-agent\"]"), "{shown}");
        assert!(shown.ends_with("Do it."), "{shown}");
    }

    #[test]
    fn list_shows_the_open_tickets_and_all_shows_the_closed_too() {
        let home = short_tempdir();
        let folder = home.path().join("tickets");
        ticket(&folder, &["new", "One", "--label", "ready-for-agent"]).unwrap();
        ticket(&folder, &["new", "Two"]).unwrap();
        ticket(&folder, &["close", "k1"]).unwrap();
        assert_eq!(ticket(&folder, &["list"]).unwrap(), ["k2  open  Two"]);
        assert_eq!(
            ticket(&folder, &["list", "--all"]).unwrap(),
            ["k1  closed  One [ready-for-agent]", "k2  open  Two"]
        );
    }

    #[test]
    fn edit_changes_what_it_is_told_to_and_nothing_else() {
        let home = short_tempdir();
        let folder = home.path().join("tickets");
        ticket(&folder, &["new", "One"]).unwrap();
        ticket(&folder, &["new", "Two", "--label", "a", "--label", "b"]).unwrap();
        let edit = [
            "edit", "k2", "--title", "Second", "--unlabel", "a", "--label", "c", "--blocked-by", "k1",
        ];
        ticket(&folder, &edit).unwrap();
        let shown = ticket(&folder, &["show", "k2"]).unwrap().join("\n");
        assert!(shown.contains("title: \"Second\""), "{shown}");
        assert!(shown.contains("labels: [\"b\", \"c\"]"), "{shown}");
        assert!(shown.contains("blocked_by: [\"k1\"]"), "{shown}");
        ticket(&folder, &["edit", "k2", "--unblock", "k1", "--parent", "k1"]).unwrap();
        let shown = ticket(&folder, &["show", "k2"]).unwrap().join("\n");
        assert!(shown.contains("parent: \"k1\"") && !shown.contains("blocked_by"), "{shown}");
        ticket(&folder, &["edit", "k2", "--no-parent"]).unwrap();
        let shown = ticket(&folder, &["show", "k2"]).unwrap().join("\n");
        assert!(!shown.contains("parent"), "{shown}");
    }

    #[test]
    fn close_can_say_why() {
        let home = short_tempdir();
        let folder = home.path().join("tickets");
        ticket(&folder, &["new", "One"]).unwrap();
        ticket(&folder, &["close", "k1", "--not-planned"]).unwrap();
        let shown = ticket(&folder, &["show", "k1"]).unwrap().join("\n");
        assert!(shown.contains("state: not_planned"), "{shown}");
    }

    #[test]
    fn a_line_that_is_not_one_of_the_verbs_is_refused_with_the_usage() {
        let home = short_tempdir();
        let folder = home.path().join("tickets");
        for args in [
            &["new"][..],
            &["new", "a", "b"],
            &["new", "a", "--body"],
            &["new", "a", "--nope", "x"],
            &["new", "a", "--parent", "k4"],
            &["show", "340"],
            &["show", "k9"],
            &["edit", "k1", "--title", "x"],
            &["list", "extra"],
            &["close"],
            &["frobnicate"],
            &[],
        ] {
            assert!(ticket(&folder, args).is_err(), "{args:?}");
        }
        let err = ticket(&folder, &["frobnicate"]).unwrap_err();
        assert!(err.starts_with("usage: shep kelpie ticket new"), "{err}");
        let err = ticket(&folder, &["show", "340"]).unwrap_err();
        assert!(err.contains("GitHub issue"), "{err}");
    }
}
