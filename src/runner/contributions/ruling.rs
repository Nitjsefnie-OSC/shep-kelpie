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

use super::super::Answer;
use super::super::ruling::question;
use crate::ports::{Finding, Timestamp};
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
    let kind = RulingKind::Contribution(Held::Security {
        head: head.to_owned(),
        lines,
    });
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
/// a followed contribution any more
pub(super) fn withdraw(next: &mut ProjectState) {
    let followed = &next.contributions;
    next.rulings.retain(|r| match &r.kind {
        RulingKind::Contribution(Held::Security { head, .. }) => {
            (followed.iter()).any(|c| c.pull_request == r.number() && c.head == *head)
        }
        _ => true,
    });
}

/// Records the head a yes to `ruling` trusts, in `next`, so that it counts as
/// passed. It trusts only the head the ruling is about, never one that came
/// after; any other answer trusts nothing.
pub(in crate::runner) fn trust(next: &mut ProjectState, answer: &Answer, ruling: &Ruling) {
    let (Answer::Yes, RulingKind::Contribution(Held::Security { head, .. })) =
        (answer, &ruling.kind)
    else {
        return;
    };
    let held = (next.contributions.iter_mut())
        .find(|c| c.pull_request == ruling.number() && c.head == *head);
    if let Some(held) = held {
        held.passed = Some(head.clone());
    }
}
