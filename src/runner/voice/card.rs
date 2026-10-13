//! A ruling's comment in one short format
//!
//! Every kind renders the same way: a title, what happened, a table of what
//! each option does, the panel's words, and short ways to answer. The same
//! options are what a reaction or a ticked box on the comment is read
//! against ([`thumbs`] and [`ticked`]), so what the comment offers and what
//! kelpie accepts cannot drift apart.
//!
//! The panel's list holds each expert's vote once the panel has voted, and
//! the option it stood behind as the recommendation. A ruling the panel has
//! not voted on holds the project manager's words where it gave any.

use super::super::Answer;
use super::super::gate::short;
use super::super::ruling::{needs_pass, stuck_comment};
use super::super::words::{NO_NOTE, Wants};
use super::merge_notes;
use crate::panel::Voter;
use crate::state::{Ruling, RulingKind, Stuck};
use crate::work_item::Phase;

/// What the project manager's `no` is in a table, when it has no more to say
const SENDS_BACK: &str = "Sends the worker back with a note, and the review reads its change again";

/// What picking an option of a worker's question does, when the worker wrote no effect
const SENDS_ANSWER: &str = "Sends the worker this answer";

/// What a no does to a work item that cannot go on
const STOPS: &str = "Stops the work item, keeping its branch and pull request on the forge";

/// What a no does to a work item over its budget
const RAISES: &str = "Sends the worker your note, and raises the work item's cap as a yes does";
const RAISES_AND_REVIEWS: &str = "Sends the worker your note, raises the work item's cap as a yes does, and has the pull request reviewed again";

/// One way to answer a ruling, as its table lists it
struct Choice {
    /// What the table calls it
    name: String,
    /// What the table says answering it does
    does: String,
    /// The answer ticking it gives
    answer: Answer,
}

/// What a ruling's comment says before it says how to answer
struct Card {
    /// What is wrong, in plain words, where the worker or the project manager
    /// wrote it: the comment opens with it
    problem: Option<String>,
    title: String,
    /// What happened, when the title leaves something unsaid
    facts: Option<String>,
    /// The options a ruling lists, A first
    choices: Vec<Choice>,
    /// What the board shows of picking an option, in the project manager's words
    board: Option<String>,
}

/// The comment a ruling is posted as, through the App
///
/// `mention` is the login whose reaction, tick or reply answers it. With none
/// the comment says only that the terminal does.
pub(super) fn ruling_comment(mention: Option<&str>, ruling: &Ruling) -> String {
    let card = Card::for_ruling(ruling);
    let mut blocks = card.opening();
    blocks.push(table(&card.choices, &ruling.kind));
    blocks.extend(card.on_the_board());
    blocks.extend(panel(ruling, &card.choices));
    blocks.extend(recommendation(ruling));
    blocks.push(match mention {
        Some(login) => answering(login, ruling.number(), &ruling.kind, &card.choices),
        None => unmentioned(ruling.number(), &ruling.kind, &card.choices),
    });
    blocks.join("\n\n")
}

/// What an expert on the panel is shown of `ruling`: its title, what
/// happened and the table of its options, as the comment opens, and how
/// many options there are
pub(in crate::runner) fn brief(ruling: &Ruling) -> (String, usize) {
    let card = Card::for_ruling(ruling);
    let mut blocks = card.opening();
    blocks.push(table(&card.choices, &ruling.kind));
    blocks.extend(card.on_the_board());
    (blocks.join("\n\n"), card.choices.len())
}

/// A ruling's comment once it is settled, to edit over the one it was posted as
///
/// `line` says how it was settled and that nothing waits on the maintainer.
/// The question stays under it, with the panel's words, and no option, tick,
/// reaction or reply is offered.
pub(in crate::runner) fn settled_comment(ruling: &Ruling, line: &str) -> String {
    let card = Card::for_ruling(ruling);
    let mut blocks = vec![line.to_owned()];
    blocks.extend(card.opening());
    blocks.extend(panel(ruling, &card.choices));
    blocks.join("\n\n")
}

/// What each option of a ruling of `kind` does when kelpie words nothing more,
/// in the order the card lists them
pub(in crate::runner) fn phrases(kind: &RulingKind) -> Vec<String> {
    (Card::of(kind).choices.into_iter())
        .map(|c| c.does)
        .collect()
}

/// The option key a `answer` is to a ruling of `kind`, if it is one of its options
///
/// A no or a rework is the option whatever its note says, and an answer in
/// words is the option that says the same.
pub(in crate::runner) fn chosen(kind: &RulingKind, answer: &Answer) -> Option<char> {
    let same = |a: &Answer, b: &Answer| match (a, b) {
        (Answer::Yes, Answer::Yes)
        | (Answer::No(_), Answer::No(_))
        | (Answer::Rework(_), Answer::Rework(_)) => true,
        (Answer::Text(a), Answer::Text(b)) => a.trim() == b.trim(),
        _ => false,
    };
    let at = Card::of(kind)
        .choices
        .iter()
        .position(|c| same(&c.answer, answer))?;
    ('A'..='Z').nth(at)
}

/// The answer option `key` of a ruling of `kind` gives, a no or a rework
/// carrying `note`; none when the ruling lists no such option
pub(in crate::runner) fn answer_for(kind: &RulingKind, key: char, note: &str) -> Option<Answer> {
    let at = ('A'..='Z').position(|letter| letter == key)?;
    let choice = Card::of(kind).choices.into_iter().nth(at)?;
    Some(match choice.answer {
        Answer::No(_) => Answer::No(note.to_owned()),
        Answer::Rework(_) => Answer::Rework(note.to_owned()),
        other => other,
    })
}

/// What option `key` of a ruling of `kind` is called in its table, if it is listed
pub(in crate::runner) fn named(kind: &RulingKind, key: char) -> Option<String> {
    let at = ('A'..='Z').position(|letter| letter == key)?;
    Card::of(kind).choices.into_iter().nth(at).map(|c| c.name)
}

/// The answer a 👍 (`up`) or a 👎 gives to a ruling of `kind`, if it takes one
pub(in crate::runner) fn thumbs(kind: &RulingKind, up: bool) -> Option<Answer> {
    let card = Card::of(kind);
    match (Wants::of(kind), up) {
        (Wants::Answer, _) => None,
        (_, true) => card.choices.into_iter().next().map(|c| c.answer),
        (_, false) => (card.choices.len() > 1).then(|| Answer::No(NO_NOTE.to_owned())),
    }
}

/// The answer the one box ticked in `body` gives to a ruling of `kind`
///
/// `None` when no box is ticked, when several are, or when the ruling lists
/// no boxes to tick.
pub(in crate::runner) fn ticked(kind: &RulingKind, body: &str) -> Option<Answer> {
    let card = Card::of(kind);
    if !boxes(kind) {
        return None;
    }
    // Only the boxes kelpie wrote count: the lines after the one that says to
    // tick, not a task line in the facts a worker's question carries.
    let lines: Vec<&str> = body.lines().collect();
    let offered = lines
        .iter()
        .rposition(|line| line.trim_end().ends_with("ick one:"))?;
    let mut ticked = lines[offered + 1..].iter().filter_map(|line| {
        let key = line.trim().strip_prefix("- [")?;
        let (mark, key) = key.split_once("] ")?;
        matches!(mark, "x" | "X").then(|| key.trim().to_owned())
    });
    let (Some(key), None) = (ticked.next(), ticked.next()) else {
        return None;
    };
    let at = ('A'..='Z').position(|letter| key == letter.to_string())?;
    card.choices.into_iter().nth(at).map(|c| c.answer)
}

// Whether the comment lists boxes to tick: a merge ruling's and a question's
// options, where a yes or a no is a reaction
fn boxes(kind: &RulingKind) -> bool {
    match Wants::of(kind) {
        Wants::Merge => true,
        Wants::Answer => !Card::of(kind).choices.is_empty(),
        Wants::YesOrNo => false,
    }
}

impl Card {
    // The card of `ruling`: its kind's, with the effect kelpie worded when it
    // raised the ruling where it worded one
    fn for_ruling(ruling: &Ruling) -> Self {
        let mut card = Self::of(&ruling.kind);
        for (choice, effect) in card.choices.iter_mut().zip(&ruling.effects) {
            if !effect.trim().is_empty() {
                choice.does.clone_from(effect);
            }
        }
        card
    }

    // What the comment opens with: the problem, then the title and the facts
    fn opening(&self) -> Vec<String> {
        let mut blocks: Vec<String> = self.problem.clone().into_iter().collect();
        blocks.push(format!("### {}", self.title));
        blocks.extend(self.facts.clone());
        blocks
    }

    // The project manager's note on what the board shows, after the table
    fn on_the_board(&self) -> Option<String> {
        let note = self.board.as_ref()?;
        Some(format!("**On the board**\n{note}"))
    }

    fn of(kind: &RulingKind) -> Self {
        match kind {
            RulingKind::Merge { head, .. } => {
                let facts = merge_notes(kind).join(" ");
                let mut choices = vec![
                    choice("Merge", "Merges it into main", Answer::Yes),
                    choice(
                        "Send back",
                        if needs_pass(kind) {
                            "Sends the worker back for a change the whole review reads again"
                        } else {
                            "Sends the worker back for a fix that goes to CI and back to you"
                        },
                        Answer::No(NO_NOTE.to_owned()),
                    ),
                ];
                if !needs_pass(kind) {
                    let does = "Sends the worker back for a change the whole review reads again";
                    choices.push(choice("Rework", does, Answer::Rework(NO_NOTE.to_owned())));
                }
                Self {
                    problem: None,
                    title: format!("Merge this pull request at {} into main?", short(head)),
                    facts: (!facts.is_empty()).then_some(facts),
                    choices,
                    board: None,
                }
            }
            RulingKind::Stuck(reason) => {
                let (title, yes, no) = stuck(reason);
                Self {
                    problem: None,
                    title: title.to_owned(),
                    facts: (!matches!(reason, Stuck::Unlabelled { .. }))
                        .then(|| stuck_comment(reason)),
                    choices: yes_no(yes, no),
                    board: None,
                }
            }
            RulingKind::Question { asked, .. } => {
                let Asking {
                    problem,
                    stem,
                    options,
                    board,
                } = split_question(asked);
                let (title, facts) = match stem.split_once('\n') {
                    Some((title, rest)) => (title.trim(), rest.trim()),
                    None => (stem.as_str(), ""),
                };
                let title = match title.is_empty() {
                    true => "The worker asks a question",
                    false => title,
                };
                Self {
                    problem,
                    title: title.to_owned(),
                    facts: (!facts.is_empty()).then(|| facts.to_owned()),
                    choices: (options.into_iter())
                        .map(|(option, effect)| {
                            let answer = Answer::Text(option.trim_matches('`').to_owned());
                            choice(&option, effect.as_deref().unwrap_or(SENDS_ANSWER), answer)
                        })
                        .collect(),
                    board,
                }
            }
            RulingKind::AgentFiles { .. } => Self {
                problem: None,
                board: None,
                title: "Accept changes to agents' own files?".to_owned(),
                facts: Some(super::said(kind)),
                choices: yes_no(
                    "Accepts the files at that head and kelpie carries on",
                    Some(STOPS),
                ),
            },
            RulingKind::ForeignChange { .. } => Self {
                problem: None,
                board: None,
                title: "Accept a change made outside kelpie?".to_owned(),
                facts: Some(super::said(kind)),
                choices: yes_no("Accepts the change and kelpie carries on", Some(SENDS_BACK)),
            },
            RulingKind::FollowUp { findings, refused } => {
                let count = match findings.len() {
                    1 => "1 unfixed finding".to_owned(),
                    n => format!("{n} unfixed findings"),
                };
                let (title, yes, left) = match refused {
                    Some(_) => (
                        format!("Try filing {count} again?"),
                        "Tries filing each finding as an issue again",
                        ", and the forge would not take them as issues",
                    ),
                    None => (
                        format!("File {count} as issues?"),
                        "Files each finding as an issue on the board",
                        "",
                    ),
                };
                let held = match findings.len() {
                    1 => "1 confirmed finding".to_owned(),
                    n => format!("{n} confirmed findings"),
                };
                Self {
                    problem: None,
                    board: None,
                    title,
                    facts: Some(format!(
                        "This pull request merged with {held} left unfixed{left}."
                    )),
                    choices: yes_no(yes, Some("Drops the findings")),
                }
            }
        }
    }
}

// The title of a stuck ruling, and what a yes does and a no does
fn stuck(reason: &Stuck) -> (&'static str, &'static str, Option<&'static str>) {
    match reason {
        Stuck::Rebase { .. } => (
            "This branch cannot be rebased onto main",
            "Kelpie looks again once you have fixed the branch",
            Some(SENDS_BACK),
        ),
        Stuck::StillRed { .. } => (
            "CI is still red. Look again?",
            "Kelpie looks at CI again",
            Some(SENDS_BACK),
        ),
        Stuck::MergeRefused { .. } => (
            "Merging was refused twice. Try again?",
            "Kelpie looks again and merges once every gate passes",
            Some(SENDS_BACK),
        ),
        Stuck::Closed => (
            "This pull request was closed. Drop the work item?",
            "Drops the work item and keeps its branch on the forge",
            Some(SENDS_BACK),
        ),
        Stuck::FixNotPushed { .. } => (
            "A fix ended without a push. Send the findings again?",
            "Sends the worker the findings again",
            Some(SENDS_BACK),
        ),
        Stuck::Unpushed { .. } => (
            "Work is not pushed. Give the worker another turn?",
            "Gives the worker another turn to push or discard its work",
            Some(SENDS_BACK),
        ),
        Stuck::TurnTimeout { .. } => (
            "The worker ran past its turn's ceiling. Resume it?",
            "Resumes the worker's session for another turn",
            Some(STOPS),
        ),
        Stuck::TurnFailed { .. } => (
            "The worker's turn failed. Try again?",
            "Tries that step again",
            Some(STOPS),
        ),
        Stuck::OverBudget {
            phase: Phase::Review(_),
            ..
        } => (
            "The work item is over its budget. Raise the cap?",
            "Doubles the work item's cap, and again until it covers the spend, and the review round starts",
            Some(RAISES_AND_REVIEWS),
        ),
        Stuck::OverBudget { .. } => (
            "The work item is over its budget. Raise the cap?",
            "Doubles the work item's cap, and again until it covers the spend, and the worker carries on",
            Some(RAISES),
        ),
        Stuck::Unlabelled { .. } => (
            "No implementer was picked",
            "The board takes the issue once it is labelled `agent:<name>`",
            None,
        ),
    }
}

fn choice(name: &str, does: &str, answer: Answer) -> Choice {
    Choice {
        name: name.to_owned(),
        does: does.to_owned(),
        answer,
    }
}

// A yes, and a no when the ruling takes one
fn yes_no(yes: &str, no: Option<&str>) -> Vec<Choice> {
    let no = no.map(|does| choice("No", does, Answer::No(NO_NOTE.to_owned())));
    [Some(choice("Yes", yes, Answer::Yes)), no]
        .into_iter()
        .flatten()
        .collect()
}

/// A worker's question, read
struct Asking {
    /// What is wrong, from its `Problem:` line
    problem: Option<String>,
    /// What it says, without the lines the worker and the project manager lead
    stem: String,
    /// The options it lists, each with the effect written after its ` => `
    options: Vec<(String, Option<String>)>,
    /// What the project manager's `PM board:` line says
    board: Option<String>,
}

// A worker's question without the project manager's lines, split into what it
// says and the options it lists, one `- ` line each at its end
fn split_question(asked: &str) -> Asking {
    let leads = |line: &str| {
        [RECOMMENDS, SAYS, PROBLEM, BOARD]
            .iter()
            .any(|l| line.starts_with(l))
    };
    let lines: Vec<&str> = asked.lines().filter(|line| !leads(line)).collect();
    let listed = |line: &&str| line.trim_start().starts_with("- ");
    let start = lines.iter().rposition(|l| !listed(l)).map_or(0, |i| i + 1);
    let (stem, list) = lines.split_at(start);
    Asking {
        problem: led_by(asked, PROBLEM),
        stem: stem.join("\n").trim().to_owned(),
        options: (list.iter())
            .map(|line| effect_of(line.trim_start().trim_start_matches("- ").trim()))
            .collect(),
        board: led_by(asked, BOARD),
    }
}

// The rest of the last line of `text` that starts with `lead`
fn led_by(text: &str, lead: &str) -> Option<String> {
    let line = text
        .lines()
        .rev()
        .find_map(|l| l.strip_prefix(lead))?
        .trim();
    (!line.is_empty()).then(|| line.to_owned())
}

/// The option a `- ` line of a question lists, and the effect written after
/// its ` => `, if one is. A ` => ` inside backticks is part of the option.
pub(in crate::runner) fn effect_of(line: &str) -> (String, Option<String>) {
    let mut ticks = 0;
    for (at, c) in line.char_indices() {
        match c {
            '`' => ticks += 1,
            '=' if ticks % 2 == 0 && line[..at].ends_with(' ') && line[at..].starts_with("=> ") => {
                let effect = line[at + 3..].trim();
                let option = line[..at].trim_end().to_owned();
                return (option, (!effect.is_empty()).then(|| effect.to_owned()));
            }
            _ => {}
        }
    }
    (line.to_owned(), None)
}

/// What leads the line where a worker says what is wrong, and where the
/// project manager rewrites it when it passes the question on
pub(in crate::runner) const PROBLEM: &str = "Problem:";

/// What leads the line where the project manager says what the board shows
pub(in crate::runner) const BOARD: &str = "PM board:";

/// What leads the project manager's pick in a worker's question
const RECOMMENDS: &str = "PM recommends:";

/// What leads the project manager's reasons, in a question and in a ruling it
/// put to the maintainer
const SAYS: &str = "PM says:";

// The panel's list: each expert's line once the panel has voted, in the
// order the table lists them; before, and in a ruling raised before the
// panel, the project manager's words where it gave any
fn panel(ruling: &Ruling, choices: &[Choice]) -> Option<String> {
    let Some(ballots) = ruling.panel.as_ref().filter(|b| !b.is_empty()) else {
        return pm_words(ruling, choices);
    };
    let lines: Vec<String> = Voter::ALL
        .into_iter()
        .filter_map(|voter| {
            let ballot = ballots.iter().find(|b| b.voter == voter)?;
            let name = voter.name();
            Some(match &ballot.vote {
                Some(vote) => format!("- **{name}** ({}): {}", vote.choice, inert(&vote.why)),
                // An abstaining project manager may still have left its words.
                None if voter == Voter::Pm => pm_words(ruling, choices)
                    .and_then(|words| words.lines().nth(1).map(str::to_owned))
                    .unwrap_or_else(|| format!("- **{name}**: abstained")),
                None => format!("- **{name}**: abstained"),
            })
        })
        .collect();
    Some(format!("**Panel**\n{}", lines.join("\n")))
}

// The option the panel stood behind, once it has voted, as the comment ends
// its list: `**Recommendation: A** (3 of 4)`.
fn recommendation(ruling: &Ruling) -> Option<String> {
    let advice = ruling.verdict.as_ref()?.advice?;
    Some(format!(
        "**Recommendation: {}** ({} of {})",
        advice.choice, advice.agree, advice.of
    ))
}

// `text` as plain words in a comment kelpie posts, so a vote a page steered
// cannot mention anyone, link anywhere or format the line: markdown's marks
// are escaped, a link's `://` and `www.` are broken and an `@` is followed by
// a zero-width space
fn inert(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' | '`' | '*' | '_' | '[' | ']' | '(' | ')' | '<' | '>' | '#' | '|' | '~' | '&'
            | '!' => {
                out.push('\\');
                out.push(c);
            }
            '@' => out.push_str("@\u{200b}"),
            _ => out.push(c),
        }
    }
    out.replace("://", ":\u{200b}//")
        .replace("www.", "www\u{200b}.")
        .replace("WWW.", "WWW\u{200b}.")
}

// The project manager's words alone, where it gave any
fn pm_words(ruling: &Ruling, choices: &[Choice]) -> Option<String> {
    let says = after_last(&ruling.question, SAYS);
    let picked = after_last(&ruling.question, RECOMMENDS).and_then(|pick| {
        let plain = |text: &str| text.trim().trim_matches('`').to_lowercase();
        let at = choices.iter().position(|c| plain(&c.name) == plain(pick))?;
        ('A'..='Z').nth(at)
    });
    let line = match (picked, says) {
        (None, None) => return None,
        (Some(key), None) => format!("- **PM** ({key})"),
        (Some(key), Some(words)) => format!("- **PM** ({key}): {words}"),
        (None, Some(words)) => format!("- **PM**: {words}"),
    };
    Some(format!("**Panel**\n{line}"))
}

// The rest of the line after the last `lead` in `text`
fn after_last<'a>(text: &'a str, lead: &str) -> Option<&'a str> {
    let at = text.rfind(lead)? + lead.len();
    let line = text[at..].lines().next()?.trim();
    (!line.is_empty()).then_some(line)
}

fn table(choices: &[Choice], kind: &RulingKind) -> String {
    let mut rows = vec![
        "| | Option | If you pick it |".to_owned(),
        "|---|---|---|".to_owned(),
    ];
    for (key, choice) in ('A'..='Z').zip(choices) {
        rows.push(format!(
            "| **{key}** | {} | {} |",
            cell(&choice.name),
            cell(&choice.does)
        ));
    }
    if Wants::of(kind) == Wants::Answer {
        rows.push("| | Other | Reply with your own answer |".to_owned());
    }
    rows.join("\n")
}

// A table cell: one line, with no bar to end it early
fn cell(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    line.replace('|', "\\|")
}

fn answering(login: &str, number: u64, kind: &RulingKind, choices: &[Choice]) -> String {
    let mut lines = vec![format!("@{login}")];
    let tick = |lines: &mut Vec<String>| {
        lines.extend(
            ('A'..='Z')
                .zip(choices)
                .map(|(key, _)| format!("- [ ] {key}")),
        );
    };
    match Wants::of(kind) {
        Wants::Merge => {
            lines.push("- 👍 or 👎 answers, or tick one:".to_owned());
            tick(&mut lines);
            lines.push(match needs_pass(kind) {
                true => format!("- Or reply `{number} no <note>`"),
                false => format!("- Or reply `{number} no <note>` or `{number} rework <note>`"),
            });
        }
        Wants::Answer => {
            let reply = format!(
                "with your answer, and start it with `{number}` if other rulings wait on this thread"
            );
            if choices.is_empty() {
                lines.push(format!("- Reply {reply}"));
            } else {
                lines.push("- Tick one:".to_owned());
                tick(&mut lines);
                lines.push(format!("- Or reply {reply}"));
            }
        }
        Wants::YesOrNo if choices.len() == 1 => {
            lines.push("- 👍 answers yes".to_owned());
            lines.push(format!("- Or reply `{number} yes`"));
        }
        Wants::YesOrNo => {
            lines.push("- 👍 answers yes, 👎 answers no".to_owned());
            lines.push(format!("- Or reply `{number} yes` or `{number} no <note>`"));
        }
    }
    lines.join("\n")
}

// A ruling that mentions no one is answered at the terminal only.
fn unmentioned(number: u64, kind: &RulingKind, choices: &[Choice]) -> String {
    let rule = format!("shep kelpie rule {number}");
    let forms = match Wants::of(kind) {
        Wants::Answer => format!("`{rule} <your answer>`"),
        Wants::Merge if needs_pass(kind) => format!("`{rule} yes` or `{rule} no <note>`"),
        Wants::Merge => format!("`{rule} yes`, `{rule} no <note>` or `{rule} rework <note>`"),
        Wants::YesOrNo if choices.len() == 1 => format!("`{rule} yes`"),
        Wants::YesOrNo => format!("`{rule} yes` or `{rule} no <note>`"),
    };
    format!(
        "This ruling mentions no one, so a reaction, a tick or a reply here answers nothing. \
         Answer it at the terminal: {forms}."
    )
}

#[cfg(test)]
mod tests;
