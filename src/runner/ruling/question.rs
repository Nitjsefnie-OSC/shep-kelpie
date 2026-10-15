//! The text of a ruling's question, which says what a yes and a no do

use super::{fix_turns_had, needs_pass};
use crate::ports::Cost;
use crate::runner::contributions::flagged_lines;
use crate::runner::gate::short;
use crate::runner::review::pushed::off_text;
use crate::state::{Fix, Held, RulingKind, Stuck};
use crate::work_item::{Phase, raised_cap};

pub(in crate::runner) fn question(issue: u64, number: Option<u64>, kind: &RulingKind) -> String {
    // The maintainer answers by the number they read: the pull request's, or
    // the issue's until there is one.
    let named = number.unwrap_or(issue);
    let trigger = |answer: &str| format!("`shep kelpie rule {named} {answer}`");
    let (yes, no) = (trigger("yes"), trigger("no <note>"));
    let about = number.map_or_else(
        || format!("issue #{issue}"),
        |n| format!("pull request #{n}"),
    );
    // A work item stopped before its pull request opened leaves none on the forge.
    let kept = if number.is_some() {
        ", keeping its branch and pull request on the forge"
    } else {
        ""
    };
    let ask = match kind {
        RulingKind::Merge {
            head,
            unreviewed,
            open_threads,
            unread_head,
            note_fix,
            late_fix,
            nits,
            disputes,
            ..
        } => {
            let mut unread = unreviewed.as_ref().map_or_else(String::new, |why| {
                format!(" No reviewer read it in its last review: {why}.")
            });
            if *unread_head {
                unread.push_str(
                    " Kelpie has no record of a review round reading this head, or of a fix turn it sent pushing it.",
                );
            }
            if *note_fix {
                unread.push_str(
                    " This head is the worker's fix for a note or comment, and no reviewer read it.",
                );
            }
            if *late_fix {
                unread.push_str(
                    " This head is the worker's fix for a review bot's late review, and no reviewer read it.",
                );
            }
            let mut open = open_threads.as_ref().map_or_else(String::new, |open| {
                format!(" Review bot threads are still open on it: {open}.")
            });
            if let Some(nits) = nits {
                open.push_str(&format!(" {nits}."));
            }
            if let Some(disputes) = disputes {
                open.push_str(&format!(" {disputes}"));
            }
            let rework = trigger("rework <note>");
            if needs_pass(kind) {
                return format!(
                    "Merge {about} at {} into main?{unread}{open} {yes} merges it, and {no} \
                     or {rework} sends the worker your note for a change the whole review \
                     reads again, since only a new pass clears that.",
                    short(head)
                );
            }
            return format!(
                "Merge {about} at {} into main?{unread}{open} {yes} merges it, {no} sends the \
                 worker your note for a fix that goes to CI and back to you, and {rework} \
                 sends it your note for a change the whole review reads again.",
                short(head)
            );
        }
        RulingKind::Stuck(reason) => match reason {
            Stuck::Rebase { why } => format!(
                "Kelpie cannot rebase {about} onto main: {why}. \
                 Once the branch is fixed, {yes} has kelpie look again"
            ),
            Stuck::StillRed {
                head,
                checks,
                fix_turns: None,
                rerun,
                ..
            } => format!(
                "CI failed again on {about} at {}, and the worker pushed \
                 no fix: {}. {yes} has kelpie look again{}",
                short(head),
                checks.join(", "),
                reruns(*rerun, &trigger("rerun"))
            ),
            Stuck::StillRed {
                head,
                checks,
                fix_turns: Some(turns),
                rerun,
                ..
            } => format!(
                "CI failed on {about} at {}: {}. {}, and the cap `ci.fix_attempts` sets is \
                 reached. {yes} has kelpie look again{}",
                short(head),
                checks.join(", "),
                fix_turns_had(*turns),
                reruns(*rerun, &trigger("rerun"))
            ),
            Stuck::MergeRefused { head, why } => format!(
                "Kelpie could not merge {about} at {} after catching it up: {why}. \
                 {yes} has kelpie look again and merge once every gate passes",
                short(head)
            ),
            Stuck::Closed => format!(
                "{} was closed without merging. {yes} drops the work \
                 item and keeps its branch on the forge",
                capitalized(&about)
            ),
            Stuck::Unpushed {
                files,
                head,
                pushed,
                review,
            } => format!(
                "The worker on {about} still has work not pushed after its turn to push or \
                 discard it: {}. Round {} of the review did not run. {yes} gives the worker \
                 another turn to push or discard it",
                off_text(files, head, pushed),
                review.round
            ),
            Stuck::FixNotPushed { fix, said, .. } => {
                let Fix::Review(review) = fix;
                let said = said
                    .as_ref()
                    .map_or_else(String::new, |said| format!(" {said}"));
                format!(
                    "The worker on {about} ended its fix for round {} of the review without \
                     pushing, so those findings still hold.{said} {yes} sends it the same fix turn \
                     again: the findings, or the reviewer's tests still failing when it was sent back to them",
                    review.round
                )
            }
            Stuck::TestStillFails {
                fix, head, tests, ..
            } => {
                return super::test_fails::question(&about, (&yes, &no), fix, head, tests);
            }
            Stuck::TurnTimeout { .. } => {
                return format!(
                    "The worker on {about} has been running past its turn's ceiling, \
                     and kelpie stopped it. {yes} resumes its session for another turn, \
                     and {no} stops the work item{kept}."
                );
            }
            Stuck::TurnFailed { why, restore, .. } => {
                let restores = match restore.is_empty() {
                    true => String::new(),
                    false => format!(
                        ", {} restores the uncommitted change to {} in the worktree and then \
                         tries it again",
                        trigger("restore"),
                        restore.join(", ")
                    ),
                };
                return format!(
                    "The worker's turn on {about} failed: {}. {yes} tries that step again\
                     {restores}, and {no} stops the work item{kept}.",
                    why.trim()
                );
            }
            Stuck::OverBudget {
                phase: Phase::Review(_),
                spent,
                cap,
                by_role,
            } => {
                return format!(
                    "The work on {about} has cost {} against its cap of {}: the worker {} and the \
                     reviewers {}, so the review round has not started. {yes} raises \
                     the cap to {} and the round starts, and {no} sends the worker your \
                     note, raises the cap the same and has the pull request reviewed again.",
                    dollars(*spent),
                    dollars(*cap),
                    dollars(by_role.worker),
                    dollars(by_role.reviewer),
                    dollars(raised_cap(*cap, *spent))
                );
            }
            Stuck::OverBudget {
                spent,
                cap,
                by_role,
                ..
            } => {
                return format!(
                    "The work on {about} has cost {} against its cap of {}: the worker {} and the \
                     reviewers {}, so the worker's next turn has not started. {yes} raises \
                     the cap to {} and the worker carries on, and {no} sends the worker your \
                     note and raises the cap the same.",
                    dollars(*spent),
                    dollars(*cap),
                    dollars(by_role.worker),
                    dollars(by_role.reviewer),
                    dollars(raised_cap(*cap, *spent))
                );
            }
            Stuck::AdoptFailed { why } => {
                return format!(
                    "Kelpie cannot start {about}, which was adopted: {}. Fix what it names, \
                     for a branch held by a worktree of yours remove that worktree or switch \
                     it off the branch, then answer {yes}. Kelpie tries again every poll \
                     and clears this once it starts.",
                    why.trim()
                );
            }
            Stuck::Unlabelled { why } => {
                return format!(
                    "The issue writer could not pick an implementer for {about}: {}. Label \
                     it `agent:<name>` for one `agents.implementers` lists, then answer {yes}: \
                     the board takes it once this is answered, and an issue still unlabelled \
                     goes to the issue writer again.",
                    why.trim()
                );
            }
        },
        RulingKind::Question { asked, .. } => {
            return format!(
                "The worker on {about} asks:\n\n{asked}\n\n{} sends the worker your answer.",
                trigger("<text>")
            );
        }
        RulingKind::AgentFiles { head, files, .. } => {
            return format!(
                "{} at {} changes agents' own files, which decide what \
                 an agent runs in the worktree: {}. {yes} accepts them at that head and kelpie \
                 carries on, and {no} stops the work item, keeping its branch and \
                 pull request on the forge.",
                capitalized(&about),
                short(head),
                files.join(", ")
            );
        }
        RulingKind::ForeignChange { description, .. } => {
            return format!(
                "{} changed outside kelpie: {description}. {yes} accepts it and kelpie \
                 carries on, and {no} sends the worker your note.",
                capitalized(&about)
            );
        }
        RulingKind::Contribution(Held::Security { head, lines }) => {
            let yes = trigger(&format!("yes {}", short(head)));
            return format!(
                "The security review flagged {about} at {}, so nothing of it runs yet:\n{}\n\
                 {yes} trusts this head and lets it go on, and {no} leaves it unpassed, so \
                 nothing runs on it and nothing is posted.",
                short(head),
                flagged_lines(lines)
            );
        }
        RulingKind::Contribution(Held::OverBudget { cap }) => {
            return format!(
                "The calls on {about} have cost more than its cap of {}, so no further call \
                 starts on it. {yes} doubles the cap, and again until it covers the spend, and \
                 {no} does the same: nothing of the contributor's runs because of either, and \
                 nothing is posted.",
                dollars(*cap)
            );
        }
        RulingKind::Contribution(Held::Question { asked }) => {
            return format!(
                "The contributor of {about} asks for a decision: {asked}\n\
                 Nothing is posted on the pull request until you answer. Answer with \
                 `shep kelpie rule {named} answer <your answer>`: the representative words it for the \
                 contributor and posts it. This decides nothing about merging, which only a \
                 merge ruling does."
            );
        }
        RulingKind::Contribution(Held::Merge { head }) => {
            return format!(
                "The reviewers' pass over {about} has ended and CI is green on {}, and nobody \
                 merges someone else's work but you, whatever `git.merging` says. {yes} merges \
                 this head and the representative thanks the contributor. {no} merges nothing: \
                 the representative words your note for the contributor and posts it, and a no \
                 with no note posts nothing. No fix turn runs for either.",
                short(head)
            );
        }
        RulingKind::Contribution(Held::Waiting) => {
            return format!(
                "The contributor of {about} said no when kelpie asked to take it over, twice, \
                 and has been quiet since. Nothing is posted on the pull request until you \
                 answer: {} keeps waiting and asks again from the start, {} adopts it \
                 anyway, and {} has the representative thank the contributor and closes it.",
                trigger("answer keep waiting"),
                trigger("answer adopt anyway"),
                trigger("answer close with thanks")
            );
        }
        RulingKind::FollowUp { findings, refused } => {
            let list: Vec<String> = findings
                .iter()
                .map(|f| match f.line {
                    0 => format!("- {} {}", f.file, f.what.trim()),
                    line => format!("- {}:{line} {}", f.file, f.what.trim()),
                })
                .collect();
            let (said, files) = match refused {
                Some(why) => (
                    format!(
                        "The forge has refused for hours to take the {} confirmed \
                         finding(s) {about} left unfixed: {}.",
                        findings.len(),
                        why.trim()
                    ),
                    "tries again",
                ),
                None => (
                    format!(
                        "{} merged with {} confirmed finding(s) left unfixed.",
                        capitalized(&about),
                        findings.len()
                    ),
                    "files each as an issue on the board",
                ),
            };
            return format!(
                "{said}\n\n{}\n\n{yes} {files}, and {} drops them.",
                list.join("\n"),
                trigger("no")
            );
        }
    };
    format!("{ask}, and {no} sends the worker your note.")
}

// What a still-red ruling that offers a rerun says it does, after what a yes does
fn reruns(offered: bool, trigger: &str) -> String {
    match offered {
        true => format!(
            ", {trigger} reruns the failed jobs of that head once more and then looks again"
        ),
        false => String::new(),
    }
}

// An amount as dollars and cents
fn dollars(cost: Cost) -> String {
    format!("${:.2}", cost.usd())
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}
