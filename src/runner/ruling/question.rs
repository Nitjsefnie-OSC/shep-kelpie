//! The text of a ruling's question, which says what a yes and a no do

use super::{fix_turns_had, needs_pass};
use crate::ports::Cost;
use crate::runner::gate::short;
use crate::runner::review::pushed::off_text;
use crate::state::{Fix, RulingKind, Stuck};
use crate::work_item::{Phase, raised_cap};

pub(in crate::runner) fn question(
    id: u64,
    issue: u64,
    number: Option<u64>,
    kind: &RulingKind,
) -> String {
    let trigger = |answer: &str| format!("`shep kelpie rule {id} {answer}`");
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
                    " This head is the worker's fix for your note, and no reviewer read it.",
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
            } => format!(
                "CI failed again on {about} at {}, and the worker pushed \
                 no fix: {}. {yes} has kelpie look again",
                short(head),
                checks.join(", ")
            ),
            Stuck::StillRed {
                head,
                checks,
                fix_turns: Some(turns),
            } => format!(
                "CI failed on {about} at {}: {}. {}, and the cap `ci.fix_attempts` sets is \
                 reached. {yes} has kelpie look again",
                short(head),
                checks.join(", "),
                fix_turns_had(*turns)
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
            Stuck::LocalModelSpilled { review, why } => format!(
                "Round {} of the review on {about} did not run: {why}. \
                 Once the model is back on the GPU, {yes} runs the round again",
                review.round
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
            Stuck::FixNotPushed { fix, .. } => {
                let Fix::Review(review) = fix;
                format!(
                    "The worker on {about} ended its fix for round {} of the review without \
                     pushing, so those findings still hold. {yes} sends it the findings again",
                    review.round
                )
            }
            Stuck::TurnTimeout { .. } => {
                return format!(
                    "The worker on {about} has been running past its turn's ceiling, \
                     and kelpie stopped it. {yes} resumes its session for another turn, \
                     and {no} stops the work item{kept}."
                );
            }
            Stuck::TurnFailed { why, .. } => {
                return format!(
                    "The worker's turn on {about} failed: {}. {yes} tries that step again, \
                     and {no} stops the work item{kept}.",
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
