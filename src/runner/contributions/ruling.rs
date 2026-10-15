//! The ruling a flagged head raises, and what answering it does
//!
//! A head the security review flagged needs the maintainer's eye before
//! anything of it runs. One ruling is raised for it, about the pull request
//! and with no work item, naming each file, line and why. A yes records the
//! head as trusted, so it counts as passed; a no leaves it unpassed. Either
//! way nothing of the contributor's runs because of the answer, and nothing
//! is posted: the ruling reaches the maintainer's webhook, `status` and
//! `shep kelpie rule` only, since a public comment would accuse the
//! contributor and tip off an attacker, and it never goes to the panel.
//!
//! A ruling about a head that is gone, because the head moved or the pull
//! request is no longer followed, is withdrawn at once, with no post. A new
//! head is read afresh.

use super::super::gate::short;
use super::super::ruling::question;
use super::super::words::NO_NOTE;
use super::super::{Answer, RuleError};
use crate::ports::{Cost, Finding, Timestamp};
use crate::settings::PerItemUsd;
use crate::state::{Asked, Held, How, ProjectState, Released, Ruled, Ruling, RulingKind, Verdict};

/// The answer to the ruling for a contributor who said no twice that waits
/// on, and asks again after
pub(in crate::runner) const KEEP_WAITING: &str = "keep waiting";

/// The answer that takes the pull request over anyway
pub(in crate::runner) const ADOPT_ANYWAY: &str = "adopt anyway";

/// The answer that thanks the contributor and closes the pull request
pub(in crate::runner) const CLOSE_WITH_THANKS: &str = "close with thanks";

/// The longest a finding's `what` or `why` is shown, in characters: the
/// reviewer wrote them from the contributor's text, and the ruling goes to
/// the webhook
const LONGEST: usize = 300;

/// Each flagged line as the maintainer reads it, one to a line: its file and
/// line, what it is and why it matters
pub(in crate::runner) fn flagged_lines(lines: &[Finding]) -> String {
    let one_line = |text: &str| {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        match text.char_indices().nth(LONGEST) {
            Some((end, _)) => format!("{}...", &text[..end]),
            None => text,
        }
    };
    let list: Vec<String> = lines
        .iter()
        .map(|f| {
            let at = match f.line {
                0 => f.file.clone(),
                line => format!("{}:{line}", f.file),
            };
            format!("- {at} {}: {}", one_line(&f.what), one_line(&f.why))
        })
        .collect();
    list.join("\n")
}

/// Adds the ruling for `pull_request`'s flagged `head` to `next`
pub(super) fn raise(
    next: &mut ProjectState,
    pull_request: u64,
    head: &str,
    lines: Vec<Finding>,
    now: Timestamp,
) {
    let held = Held::Security {
        head: head.to_owned(),
        lines,
    };
    raise_held(next, pull_request, held, now);
}

/// Adds the ruling that asks whether `pull_request`, whose calls cost more
/// than `cap`, may go on, to `next`
pub(super) fn raise_over_budget(
    next: &mut ProjectState,
    pull_request: u64,
    cap: Cost,
    now: Timestamp,
) {
    raise_held(next, pull_request, Held::OverBudget { cap }, now);
}

/// Adds the ruling that puts the contributor's request for a decision, as
/// `asked`, to the maintainer, to `next`
pub(super) fn raise_question(
    next: &mut ProjectState,
    pull_request: u64,
    asked: String,
    now: Timestamp,
) {
    raise_held(next, pull_request, Held::Question { asked }, now);
}

/// Adds the ruling that asks what to do about a contributor who said no
/// twice and went quiet, to `next`
pub(super) fn raise_waiting(next: &mut ProjectState, pull_request: u64, now: Timestamp) {
    raise_held(next, pull_request, Held::Waiting, now);
}

/// Adds the ruling that asks whether `pull_request`'s `head` is merged to
/// `next`, with `facts` after its question: what the reviewers found
pub(super) fn raise_merge(
    next: &mut ProjectState,
    pull_request: u64,
    head: &str,
    facts: &str,
    now: Timestamp,
) {
    let held = Held::Merge {
        head: head.to_owned(),
    };
    raise_with(next, pull_request, held, facts, now);
}

fn raise_held(next: &mut ProjectState, pull_request: u64, held: Held, now: Timestamp) {
    raise_with(next, pull_request, held, "", now);
}

// The merge ruling is the one a contribution can go without, so another one
// takes its place, and it is raised again once that is answered.
fn raise_with(next: &mut ProjectState, pull_request: u64, held: Held, facts: &str, now: Timestamp) {
    next.rulings.retain(|r| {
        r.id != pull_request || !matches!(r.kind, RulingKind::Contribution(Held::Merge { .. }))
    });
    let kind = RulingKind::Contribution(held);
    let mut question = question(pull_request, Some(pull_request), &kind);
    if !facts.is_empty() {
        question.push('\n');
        question.push_str(facts);
    }
    next.rulings.push(Ruling {
        id: pull_request,
        issue: None,
        question,
        pull_request: Some(pull_request),
        kind,
        alerted: false,
        panel: None,
        verdict: None,
        unposted: false,
        skipped: None,
        effects: Vec::new(),
        raised: Some(now),
    });
}

/// Takes out of `next` each contribution ruling whose head is not the head of
/// a followed contribution any more, and each over-budget or decision one
/// whose pull request is not followed any more
pub(super) fn withdraw(next: &mut ProjectState) {
    let followed = &next.contributions;
    next.rulings.retain(|r| match &r.kind {
        RulingKind::Contribution(Held::Security { head, .. }) => {
            (followed.iter()).any(|c| c.pull_request == r.number() && c.head == *head)
        }
        RulingKind::Contribution(Held::Merge { head }) => {
            (followed.iter()).any(|c| c.pull_request == r.number() && c.head == *head)
        }
        RulingKind::Contribution(
            Held::OverBudget { .. } | Held::Question { .. } | Held::Waiting,
        ) => (followed.iter()).any(|c| c.pull_request == r.number()),
        _ => true,
    });
}

/// Lifts the cap of the contribution `ruling` is about, in `next`, for an
/// answer to its over-budget ruling, and says whether it did
///
/// A yes does, and so does a no with a note, as for a work item; each doubles
/// the cap, and again while it is below what the calls have cost, so one answer
/// always lets the next call start.
pub(in crate::runner) fn raise_budget(
    next: &mut ProjectState,
    answer: &Answer,
    ruling: &Ruling,
    per_item: PerItemUsd,
) -> bool {
    let noted = match answer {
        Answer::Yes => true,
        Answer::No(note) => !note.trim().is_empty(),
        _ => false,
    };
    if !noted
        || !matches!(
            ruling.kind,
            RulingKind::Contribution(Held::OverBudget { .. })
        )
    {
        return false;
    }
    let followed = (next.contributions.iter_mut()).find(|c| c.pull_request == ruling.number());
    if let Some(followed) = followed {
        followed.raise_budget(per_item);
    }
    true
}

/// Keeps the maintainer's answer to a contributor's request for a decision,
/// in `next`, for the representative to word and post
///
/// Only a text answer to a decision ruling is kept; any other answer to it
/// is refused by the ruling, which stays.
pub(in crate::runner) fn keep_answer(next: &mut ProjectState, answer: &Answer, ruling: &Ruling) {
    let (Answer::Text(text), RulingKind::Contribution(Held::Question { .. })) =
        (answer, &ruling.kind)
    else {
        return;
    };
    let followed = (next.contributions.iter_mut()).find(|c| c.pull_request == ruling.number());
    if let Some(deciding) = followed.and_then(|c| c.deciding.as_mut()) {
        deciding.answer = Some(text.trim().to_owned());
    }
}

/// Keeps the maintainer's answer to a contribution's merge ruling, in `next`,
/// for the next poll to carry out: a yes merges the head, and a no with a
/// note has the representative post it for the contributor
///
/// A no with no note, or the note a reaction or a tick gives, posts nothing.
pub(in crate::runner) fn rule_merge(next: &mut ProjectState, answer: &Answer, ruling: &Ruling) {
    let RulingKind::Contribution(Held::Merge { head }) = &ruling.kind else {
        return;
    };
    let verdict = match answer {
        Answer::Yes => Verdict::Yes {
            merged: false,
            thanked: false,
        },
        Answer::No(note) => {
            let note = note.trim();
            Verdict::No {
                note: (!note.is_empty() && note != NO_NOTE).then(|| note.to_owned()),
            }
        }
        _ => return,
    };
    let followed = (next.contributions.iter_mut())
        .find(|c| c.pull_request == ruling.number() && c.head == *head);
    if let Some(followed) = followed {
        followed.ruled = Some(Ruled {
            head: head.clone(),
            verdict,
        });
    }
}

/// Carries the maintainer's answer to the ruling for a contributor who said
/// no twice into `next`: keeping waiting starts the wait and the count over,
/// adopting anyway records a release by the maintainer, and closing with
/// thanks is left for the next poll, which has the representative say it
///
/// # Errors
///
/// [`RuleError::Choice`] when the text is none of the three.
pub(in crate::runner) fn settle(
    next: &mut ProjectState,
    answer: &Answer,
    ruling: &Ruling,
    now: Timestamp,
) -> Result<(), RuleError> {
    let (Answer::Text(text), RulingKind::Contribution(Held::Waiting)) = (answer, &ruling.kind)
    else {
        return Ok(());
    };
    let words = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let chosen = words.to_ascii_lowercase();
    let followed = (next.contributions.iter_mut()).find(|c| c.pull_request == ruling.number());
    match chosen.as_str() {
        KEEP_WAITING | "keep" => {
            if let Some(held) = followed {
                held.asked = None;
                held.waiting_since = Some(now);
            }
        }
        ADOPT_ANYWAY | "adopt" => {
            if let Some(held) = followed {
                held.released = Some(Released {
                    at: now,
                    how: How::Maintainer,
                });
            }
        }
        CLOSE_WITH_THANKS | "close" => {
            if let Some(held) = followed {
                let asked = held.asked.get_or_insert_with(|| Asked {
                    comment: 0,
                    at: now,
                    asks: 0,
                    nos: 0,
                    read: Vec::new(),
                    closing: false,
                    thanked: false,
                });
                asked.closing = true;
            }
        }
        _ => {
            let why = format!(
                "takes `{KEEP_WAITING}`, `{ADOPT_ANYWAY}` or `{CLOSE_WITH_THANKS}`, not {words:?}"
            );
            return Err(RuleError::Choice(ruling.id, why));
        }
    }
    Ok(())
}

/// The fewest characters of a head's hash a yes may name
const LEAST: usize = 7;

/// Records the head a yes to `ruling` trusts, in `next`, so that it counts as
/// passed
///
/// A yes names the head the maintainer read, because every ruling on a pull
/// request has its number and a yes meant for one head must not trust the
/// next. It trusts only the head the ruling is about; any other answer trusts
/// nothing.
///
/// # Errors
///
/// [`RuleError::Head`] when a yes to a contribution's ruling names no head or
/// another, or names a head of a ruling that is not one.
pub(in crate::runner) fn trust(
    next: &mut ProjectState,
    answer: &Answer,
    ruling: &Ruling,
) -> Result<(), RuleError> {
    let id = ruling.id;
    let held = match (&ruling.kind, answer) {
        (RulingKind::Contribution(Held::Security { head, .. }), Answer::Yes) => {
            let why = format!("trusts a head, so a yes names it: `yes {}`", short(head));
            return Err(RuleError::Head(id, why));
        }
        (RulingKind::Contribution(Held::Security { head, .. }), Answer::Trust(named)) => {
            let named = named.to_ascii_lowercase();
            if named.len() < LEAST || !head.starts_with(&named) {
                let why = format!(
                    "is about the head {} now, which a yes must name, as {LEAST} characters or \
                     more of its hash: `yes {}`",
                    short(head),
                    short(head)
                );
                return Err(RuleError::Head(id, why));
            }
            head
        }
        (_, Answer::Trust(_)) => {
            let why = "trusts no head, so a yes is only `yes`".to_owned();
            return Err(RuleError::Head(id, why));
        }
        _ => return Ok(()),
    };
    let followed = (next.contributions.iter_mut())
        .find(|c| c.pull_request == ruling.number() && c.head == *held);
    if let Some(followed) = followed {
        followed.passed = Some(held.clone());
    }
    Ok(())
}
