//! What the worker said of the review bot threads a fix turn was sent

use serde::{Deserialize, Serialize};

use super::WorkItem;

/// What the worker's reply said of one thread sent to it
///
/// A thread its reply did not mention has none, and kelpie replies that it
/// was not addressed.
// wire format: changing this is a breaking change to the state file
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "said", rename_all = "lowercase", deny_unknown_fields)]
pub enum ThreadAnswer {
    /// The worker fixed it
    Fixed {
        /// The forge's id of the thread
        thread: String,
    },
    /// The worker left it, for this reason
    Declined {
        /// The forge's id of the thread
        thread: String,
        /// Why, in the worker's words
        reason: String,
    },
}

impl ThreadAnswer {
    /// The forge's id of the thread answered
    #[must_use]
    pub fn thread(&self) -> &str {
        match self {
            Self::Fixed { thread } | Self::Declined { thread, .. } => thread,
        }
    }
}

/// What one line of a worker's reply says of a thread, which it numbers
/// from 1 in the order the findings file lists them
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    /// `thread 2: fixed`
    Fixed,
    /// `thread 3: declined: <reason>`
    Declined(String),
}

/// The threads `reply` speaks of, as `(number, what it says)`, in the order
/// the reply gives them
///
/// A line is `thread <n>: fixed` or `thread <n>: declined: <reason>`, in
/// any case, which a list marker or backticks around may lead and follow.
/// Any other line is the worker talking, and is skipped. A thread named
/// twice keeps its last line.
#[must_use]
pub fn thread_lines(reply: &str) -> Vec<(usize, Said)> {
    let mut lines: Vec<(usize, Said)> = Vec::new();
    for (number, said) in reply.lines().filter_map(thread_line) {
        lines.retain(|(n, _)| *n != number);
        lines.push((number, said));
    }
    lines
}

fn thread_line(line: &str) -> Option<(usize, Said)> {
    const MARKS: [char; 4] = ['-', '*', '`', ' '];
    let line = line.trim().trim_start_matches(MARKS);
    let rest = strip_word(line, "thread")?.trim_start();
    let digits = rest
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(rest.len());
    let number: usize = rest[..digits].parse().ok()?;
    let rest = rest[digits..].trim_start().strip_prefix(':')?;
    let rest = rest.trim().trim_start_matches(['*', '`', ' ']);
    let rest = rest.trim_end_matches(['*', '`']).trim();
    if let Some(after) = strip_word(rest, "declined") {
        let reason = after.trim_start_matches([':', '-', ' ']).trim();
        let reason = match reason.is_empty() {
            true => "no reason given",
            false => reason,
        };
        return Some((number, Said::Declined(reason.to_owned())));
    }
    // A thread is resolved on this word, so nothing but closing marks may
    // follow it: "fixed, no wait" and "fixed in part" are not a fix.
    let closing = |c: char| matches!(c, '.' | '!' | '*' | '`' | ' ');
    let after = strip_word(rest, "fixed")?;
    after.chars().all(closing).then_some((number, Said::Fixed))
}

// `text` without `word` at its start, whatever the case, when the word ends
// there: `fixed.` and `fixed` match `fixed`, `fixedly` does not.
fn strip_word<'a>(text: &'a str, word: &str) -> Option<&'a str> {
    let head = text.get(..word.len())?;
    let rest = &text[word.len()..];
    let ends = rest.chars().next().is_none_or(|c| !c.is_alphanumeric());
    (head.eq_ignore_ascii_case(word) && ends).then_some(rest)
}

impl WorkItem {
    /// Takes what the worker's `reply` says of the threads sent into
    /// `thread_answers`, a thread named by a later reply taking its line
    ///
    /// `threads_sent` is in the order the findings file lists them, which
    /// the reply's numbers follow. A number past the last thread is skipped.
    pub fn answer_threads(&mut self, reply: &str) {
        for (number, said) in thread_lines(reply) {
            let Some(thread) = number
                .checked_sub(1)
                .and_then(|at| self.threads_sent.get(at))
            else {
                continue;
            };
            let thread = thread.clone();
            self.thread_answers.retain(|a| a.thread() != thread);
            self.thread_answers.push(match said {
                Said::Fixed => ThreadAnswer::Fixed { thread },
                Said::Declined(reason) => ThreadAnswer::Declined { thread, reason },
            });
        }
    }

    /// What the worker said of `thread`, if it did
    #[must_use]
    pub fn answer_of(&self, thread: &str) -> Option<&ThreadAnswer> {
        self.thread_answers.iter().find(|a| a.thread() == thread)
    }

    /// Forgets `thread`, which kelpie has answered on the forge
    pub fn thread_answered(&mut self, thread: &str) {
        self.threads_sent.retain(|t| t != thread);
        self.thread_answers.retain(|a| a.thread() != thread);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declined(reason: &str) -> Said {
        Said::Declined(reason.to_owned())
    }

    // The shape of a worker's final message, as `result` carries it in
    // fixtures/claude-p-result.json. Written by hand, not recorded: no
    // recording of a worker answering this prompt exists yet.
    const REPLY: &str = "Round 3 is done and pushed.\n\n\
        I renamed the flag and guarded the index. The comment is left because the \
        caller checks the slice.\n\n\
        thread 1: fixed\n\
        thread 2: fixed\n\
        thread 3: declined: the caller guarantees a non-empty slice, so the guard is dead code";

    #[test]
    fn a_reply_ending_in_one_line_per_thread_is_read() {
        assert_eq!(
            thread_lines(REPLY),
            [
                (1, Said::Fixed),
                (2, Said::Fixed),
                (
                    3,
                    declined("the caller guarantees a non-empty slice, so the guard is dead code")
                ),
            ]
        );
    }

    #[test]
    fn a_line_in_a_list_or_in_backticks_or_bold_is_still_a_line() {
        let reply = "- thread 1: fixed\n\
                     * `thread 2: declined: it is deliberate`\n\
                     **Thread 3: Fixed.**\n\
                     Thread 4 : declined - not worth the churn";
        assert_eq!(
            thread_lines(reply),
            [
                (1, Said::Fixed),
                (2, declined("it is deliberate")),
                (3, Said::Fixed),
                (4, declined("not worth the churn")),
            ]
        );
    }

    #[test]
    fn prose_that_only_mentions_a_thread_is_not_a_line() {
        let reply = "I looked at thread 1: fixed it by renaming.\n\
                     thread one: fixed\n\
                     thread 2 fixed\n\
                     thread 3: fixedly done\n\
                     thread 4: maybe\n\
                     thread: fixed";
        assert_eq!(thread_lines(reply), []);
    }

    // A thread is resolved on this word alone, so a line that says more than
    // `fixed` is not read as it, whatever it starts with.
    #[test]
    fn a_fixed_line_that_takes_the_fix_back_is_not_read_as_fixed() {
        let reply = "thread 1: fixed, no wait, I could not\n\
                     thread 2: fixed? not really\n\
                     thread 3: fixed - but only in part\n\
                     thread 4: fixed in a1b2c3d\n\
                     thread 5: Fixed!\n\
                     thread 6: **fixed**.";
        assert_eq!(thread_lines(reply), [(5, Said::Fixed), (6, Said::Fixed)]);
    }

    #[test]
    fn a_decline_with_no_reason_says_so() {
        assert_eq!(
            thread_lines("thread 1: declined"),
            [(1, declined("no reason given"))]
        );
        assert_eq!(
            thread_lines("thread 1: declined:   "),
            [(1, declined("no reason given"))]
        );
    }

    #[test]
    fn a_thread_named_twice_keeps_its_last_line() {
        assert_eq!(
            thread_lines("thread 1: declined: later\nthread 2: fixed\nthread 1: fixed"),
            [(2, Said::Fixed), (1, Said::Fixed)]
        );
    }

    fn item_sent(threads: &[&str]) -> WorkItem {
        let mut item = crate::test::a_work_item();
        item.threads_sent = threads.iter().map(|t| (*t).to_owned()).collect();
        item
    }

    #[test]
    fn a_number_names_the_thread_in_that_place_and_one_past_the_last_names_none() {
        let mut item = item_sent(&["PRRT_a", "PRRT_b"]);
        item.answer_threads("thread 2: declined: no\nthread 3: fixed\nthread 0: fixed");
        assert_eq!(
            item.thread_answers,
            [ThreadAnswer::Declined {
                thread: "PRRT_b".into(),
                reason: "no".into()
            }]
        );
        assert_eq!(item.answer_of("PRRT_a"), None);
    }

    #[test]
    fn a_later_reply_takes_over_the_threads_it_names_and_keeps_the_rest() {
        let mut item = item_sent(&["PRRT_a", "PRRT_b"]);
        item.answer_threads("thread 1: declined: later\nthread 2: fixed");
        item.answer_threads("thread 1: fixed");
        assert_eq!(
            item.thread_answers,
            [
                ThreadAnswer::Fixed {
                    thread: "PRRT_b".into()
                },
                ThreadAnswer::Fixed {
                    thread: "PRRT_a".into()
                },
            ]
        );
    }
}
