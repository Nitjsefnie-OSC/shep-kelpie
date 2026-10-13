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
use super::super::{Answer, RuleError};
use crate::ports::{Cost, Finding, Timestamp};
use crate::settings::PerItemUsd;
use crate::state::{Held, ProjectState, Ruling, RulingKind};

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

fn raise_held(next: &mut ProjectState, pull_request: u64, held: Held, now: Timestamp) {
    let kind = RulingKind::Contribution(held);
    next.rulings.push(Ruling {
        id: pull_request,
        issue: None,
        question: question(pull_request, Some(pull_request), &kind),
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
/// a followed contribution any more, and each over-budget one whose pull
/// request is not followed any more
pub(super) fn withdraw(next: &mut ProjectState) {
    let followed = &next.contributions;
    next.rulings.retain(|r| match &r.kind {
        RulingKind::Contribution(Held::Security { head, .. }) => {
            (followed.iter()).any(|c| c.pull_request == r.number() && c.head == *head)
        }
        RulingKind::Contribution(Held::OverBudget { .. }) => {
            (followed.iter()).any(|c| c.pull_request == r.number())
        }
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
    let (Answer::Yes | Answer::No(_), RulingKind::Contribution(Held::OverBudget { .. })) =
        (answer, &ruling.kind)
    else {
        return false;
    };
    let followed = (next.contributions.iter_mut()).find(|c| c.pull_request == ruling.number());
    if let Some(followed) = followed {
        followed.raise_budget(per_item);
    }
    true
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
