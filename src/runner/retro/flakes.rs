//! Tests that fail CI on work items whose changes they have nothing to do with
//!
//! A red run whose log names no file the branch changed is journaled with the
//! tests that log shows failing. A test journaled for two work items is a
//! known flake: the retro's digest lists it, and a fix turn's prompt carries
//! it, so two workers do not both fix the same one.

use std::collections::{BTreeMap, BTreeSet};

use super::journal::{self, Entry, Stamped};
use crate::runner::Runner;

/// How many seconds of journal the fix turns' list reads: a flake that has
/// not failed for this long is let go
const WINDOW: u64 = 7 * 24 * 3600;

/// A test that failed outside the changes of two or more work items
#[derive(Debug, PartialEq, Eq)]
pub(in crate::runner) struct Flake {
    pub(super) test: String,
    /// The work items it failed on, lowest first
    pub(super) issues: Vec<u64>,
}

/// The known flakes among `entries`, the most widespread first
pub(in crate::runner) fn known<'a>(entries: impl Iterator<Item = &'a Entry>) -> Vec<Flake> {
    let mut seen: BTreeMap<&str, BTreeSet<u64>> = BTreeMap::new();
    for entry in entries {
        if let Entry::CiOutsideDiff { issue, tests } = entry {
            for test in tests {
                seen.entry(test).or_default().insert(*issue);
            }
        }
    }
    let mut flakes: Vec<Flake> = (seen.into_iter())
        .filter(|(_, issues)| issues.len() >= 2)
        .map(|(test, issues)| Flake {
            test: test.to_owned(),
            issues: issues.into_iter().collect(),
        })
        .collect();
    flakes.sort_by_key(|f| std::cmp::Reverse(f.issues.len()));
    flakes
}

/// The tests a Rust test run's `log` shows failing, in the order it first
/// shows them
///
/// Read from `test <name> ... FAILED`, `---- <name> stdout ----` and
/// `thread '<name>' panicked`, wherever on the line they sit, since the
/// forge prefixes each line.
pub(in crate::runner) fn failed_tests(log: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for line in log.lines() {
        let found = [
            between(line, "test ", " ... FAILED", true),
            between(line, "---- ", " stdout ----", false),
            between(line, "thread '", "' panicked", false),
        ];
        for name in found.into_iter().flatten() {
            let named = !name.is_empty() && !name.contains(char::is_whitespace);
            if named && name != "main" && name != "<unnamed>" && !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

// The text between `open` and the first `close` after it. With `last`, the
// open that is nearest the close, for a line a prefix comes before.
fn between(line: &str, open: &str, close: &str, last: bool) -> Option<String> {
    let end = line.find(close)?;
    let head = &line[..end];
    let from = match last {
        true => head.rfind(open)?,
        false => head.find(open)?,
    };
    Some(head[from + open.len()..].to_owned())
}

/// What a fix turn's prompt says of `flakes`, or nothing with none
pub(in crate::runner) fn note(flakes: &[Flake]) -> String {
    if flakes.is_empty() {
        return String::new();
    }
    let listed: Vec<&str> = flakes.iter().map(|f| f.test.as_str()).collect();
    format!(
        " Known flakes: these tests failed CI on two or more work items with logs that \
         named none of their changes, and another worker is not to be sent after them \
         twice: {}. If the failure here is one of these and your branch did not touch it, \
         do not fix it; name it in your final message.",
        listed.join(", ")
    )
}

impl Runner {
    /// Journals the tests `log` shows failing on the work item `issue`, as
    /// failing outside its changes
    pub(in crate::runner) fn journal_outside_diff(&mut self, issue: u64, log: &str) {
        let tests = failed_tests(log);
        if !tests.is_empty() {
            self.journal(Entry::CiOutsideDiff { issue, tests });
        }
    }

    /// The fix turns' note of the known flakes of the last [`WINDOW`], empty
    /// with none
    pub(in crate::runner) fn flake_note(&self) -> String {
        let now = self.ports.clock.now().0;
        let entries: Vec<Stamped> = journal::read(&self.paths.journal);
        let recent = (entries.iter())
            .filter(|s| s.at.0.saturating_add(WINDOW) >= now)
            .map(|s| &s.entry);
        note(&known(recent))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outside(issue: u64, tests: &[&str]) -> Entry {
        Entry::CiOutsideDiff {
            issue,
            tests: tests.iter().map(|t| (*t).to_owned()).collect(),
        }
    }

    #[test]
    fn a_test_is_read_from_each_way_a_rust_run_names_a_failure() {
        let log = "job\tstep\t2026-10-10T01:02:03Z test sheep::tests::a ... FAILED\n\
                   ---- sheep::tests::b stdout ----\n\
                   thread 'sheep::tests::c' panicked at src/sheep.rs:4:9:\n\
                   thread 'main' panicked at src/main.rs:1:1:\n\
                   test sheep::tests::a ... FAILED\n\
                   test result: FAILED. 3 passed; 1 failed\n";

        assert_eq!(
            failed_tests(log),
            ["sheep::tests::a", "sheep::tests::b", "sheep::tests::c"]
        );
    }

    #[test]
    fn a_log_with_no_test_in_it_names_none() {
        assert_eq!(
            failed_tests("error: could not compile `shep-kelpie`"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_test_on_one_work_item_is_not_a_flake_and_one_on_two_is() {
        let entries = [
            outside(3, &["a::flaky", "b::once"]),
            outside(4, &["a::flaky"]),
            outside(4, &["a::flaky"]),
        ];

        let flakes = known(entries.iter());

        assert_eq!(
            flakes,
            [Flake {
                test: "a::flaky".into(),
                issues: vec![3, 4]
            }]
        );
    }
}
