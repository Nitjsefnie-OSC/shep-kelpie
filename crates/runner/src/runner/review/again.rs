//! Which findings in a fix turn's journal entry a reviewer raised again

use crate::ports::Finding;
use crate::runner::retro::journal::Held;
use crate::work_item::WorkItem;

/// What the journal keeps of `findings`, about to be sent, each marked if
/// `item` was already sent it
///
/// An earlier round sending the same finding is a sign its fix did not fix
/// it. Held findings leave out nits, so a nit is never marked. Called before
/// the round's findings are recorded as held.
pub(super) fn kept(item: &WorkItem, findings: &[Finding]) -> Vec<Held> {
    (findings.iter())
        .map(|f| Held::of(f).again(item.held.iter().any(|known| known.is_same_as(f))))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use crate::ports::{Finding, Severity};
    use crate::runner::retro::journal::{self, Entry, Held};
    use crate::runner::{Runner, step};
    use crate::test::{Rig, Scripted, ScriptedRound, ticket};

    fn at(severity: Severity, file: &str, line: u32, what: &str) -> Finding {
        Finding::new(severity, file, line, what, "why")
    }

    // What the claude reviewer writes for `f`.
    fn line_of(f: &Finding) -> String {
        let severity = if f.is_nit() { "LOW" } else { "MEDIUM" };
        format!("{severity}|{}:{}|{}|{}\n", f.file, f.line, f.what, f.why)
    }

    // Round 1 (qwen) found `first` and its fix turn pushed; round 2 (claude)
    // found `second` and its findings went to the worker.
    fn two_rounds(first: Vec<Finding>, second: &[Finding]) -> (Rig, Mutex<Runner>) {
        let rig = Rig::new("shep");
        let runner = rig.open().unwrap();
        rig.ask(&runner, "add", Some("7"));
        rig.forge.open_pull_request(71, "kelpie/7", &[7]);
        rig.claude.script([Scripted::Push("work.txt", "work\n")]);
        step(&runner).unwrap(); // opens the pull request, enters round 1 (qwen)
        rig.reviewer.script([ScriptedRound::Findings(first)]);
        step(&runner).unwrap(); // round 1's qwen call
        step(&runner).unwrap(); // the findings go out
        rig.claude.script([Scripted::Push("fixed.txt", "fixed\n")]);
        step(&runner).unwrap(); // the fix turn pushes
        step(&runner).unwrap(); // and settles
        // The script wants a 'static text; a test's leak is its own.
        let text: String = second.iter().map(line_of).collect();
        rig.claude.script([Scripted::Text(text.leak())]);
        step(&runner).unwrap(); // round 2, claude
        step(&runner).unwrap(); // the findings go out
        (rig, runner)
    }

    // What each round's `Entry::Findings` kept, oldest round first.
    fn kept(rig: &Rig) -> Vec<Vec<Held>> {
        journal::read(&rig.paths().journal)
            .into_iter()
            .filter_map(|s| match s.entry {
                Entry::Findings {
                    issue, findings, ..
                } => {
                    assert_eq!(issue, ticket(7));
                    Some(findings)
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_finding_round_2_sends_again_is_marked_and_a_new_one_is_not() {
        let old = at(Severity::Medium, "src/a.rs", 4, "the flag is read first");
        let new = at(Severity::Medium, "src/b.rs", 9, "the lock is not released");
        let (rig, _runner) = two_rounds(vec![old.clone()], &[old.clone(), new.clone()]);

        assert_eq!(
            kept(&rig),
            [
                vec![Held::of(&old)],
                vec![Held::of(&old).again(true), Held::of(&new)]
            ]
        );
    }

    #[test]
    fn a_nit_sent_twice_is_not_marked() {
        let nit = at(Severity::Low, "src/a.rs", 4, "unused variable");
        let (rig, _runner) = two_rounds(vec![nit.clone()], std::slice::from_ref(&nit));

        assert_eq!(kept(&rig), [vec![Held::of(&nit)], vec![Held::of(&nit)]]);
    }
}
