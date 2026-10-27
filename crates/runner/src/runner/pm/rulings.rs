//! The rulings the project manager answers for the maintainer
//!
//! A ruling whose kind `rulings.pm_decides` sets `true` wakes the project
//! manager. Its comment waits for the answer, at most one wake. An answer
//! that picks an option is applied at once. It is posted headed "Answered by
//! the project manager", where a `no <note>` reopens it. An abstention, a
//! `design` flag or a merge under `ask` goes to the maintainer. The card
//! shows the project manager's pick.

use std::collections::BTreeMap;

use super::act::{WORDS, plain};
use crate::pm_answer::PmAnswer;
use crate::runner::Runner;
use crate::runner::ruling;
use crate::runner::thread_replies::Settler;
use crate::runner::voice::{RECOMMENDS, SAYS, answer_for, brief, named, settled_comment};
use crate::settings::{Merging, RulingName};
use crate::state::{Ruling, RulingKind, Stuck};
use crate::ticket::TicketId;

impl Runner {
    /// Whether the project manager answers `ruling` for the maintainer
    ///
    /// Its kind is one `rulings.pm_decides` gives it, and it lists two options
    /// or more. A question and a follow-up have wakes of their own. A
    /// contribution's ruling is never the project manager's. A turn kelpie
    /// could not set up is the maintainer's alone. So is a failed turn the
    /// project manager already answered on the same work item. In a drain it
    /// goes to the maintainer.
    pub(super) fn pm_answers_ruling(&self, ruling: &Ruling) -> bool {
        let own_wake = matches!(
            ruling.kind,
            RulingKind::Question { .. } | RulingKind::FollowUp { .. }
        );
        let setup = matches!(
            ruling.kind,
            RulingKind::Stuck(Stuck::TurnFailed { setup: true, .. })
        );
        let answers = self.agents.pm.is_some()
            && !own_wake
            && !setup
            && !ruling.kind.is_contribution()
            && !self.draining()
            && self
                .settings
                .rulings
                .pm_decides(RulingName::of(&ruling.kind))
            && brief(ruling).1 >= 2;
        answers && !self.pm_answered_before(ruling)
    }

    // Whether the project manager already answered the failed turn `ruling`
    // is, on the same work item. That answer did not settle it.
    fn pm_answered_before(&self, ruling: &Ruling) -> bool {
        let records = ruling::records(&self.paths.rulings);
        records.iter().any(|r| r.decided_by_pm(ruling))
    }

    /// How many rulings the project manager answered, by kind, for
    /// `status`; none read for a project with no project manager
    pub(in crate::runner) fn pm_answer_counts(&self) -> BTreeMap<String, u32> {
        match self.agents.pm {
            Some(_) => ruling::pm_answers(&ruling::records(&self.paths.rulings)),
            None => BTreeMap::new(),
        }
    }

    /// The option count of pending ruling `id`, for reading an answer to it
    pub(super) fn ruling_options(&self, id: TicketId) -> usize {
        (self.state.rulings.iter().find(|r| r.id == id)).map_or(0, |r| brief(r).1)
    }

    /// Hears the project manager's answers to the rulings in `asked`, and
    /// says what each did, for the log
    ///
    /// A ruling settled or gone meanwhile takes none, and one whose comment
    /// went out before the answer came is the maintainer's.
    pub(super) fn pm_answered(
        &mut self,
        asked: &[TicketId],
        answers: Vec<(TicketId, Result<PmAnswer, String>)>,
    ) -> Vec<String> {
        let mut answers: BTreeMap<TicketId, Result<PmAnswer, String>> =
            answers.into_iter().collect();
        let mut done = Vec::new();
        for &id in asked {
            self.pm.asking.remove(&id);
            let Some(ruling) = self.state.rulings.iter().find(|r| r.id == id).cloned() else {
                continue;
            };
            if !ruling.unposted || ruling.alerted {
                done.push(format!(
                    "left ruling {id}: it already went to the maintainer"
                ));
                continue;
            }
            let answer = (answers.remove(&id))
                .unwrap_or_else(|| Err("its reply held no answer to it".into()));
            let answer = match answer {
                Ok(answer) => answer,
                Err(why) => {
                    done.push(format!("gave no answer to ruling {id}: {why}"));
                    continue;
                }
            };
            let answer = PmAnswer {
                why: plain(&self.shown(answer.why), WORDS),
                ..answer
            };
            let choice = answer.choice;
            match self.pm_decides(&ruling, &answer) {
                Ok(()) => done.push(format!("answered ruling {id} with option {choice}")),
                Err(why) => {
                    self.pm_recommends(id, &answer);
                    done.push(format!(
                        "put ruling {id} to the maintainer, recommending option {choice}, since \
                         {why}"
                    ));
                }
            }
        }
        done
    }

    // Applies option `answer.choice` to `ruling`, and posts what was decided,
    // or says why the maintainer decides instead. A ruling whose answer
    // cannot be applied stays pending, for the maintainer.
    fn pm_decides(&mut self, ruling: &Ruling, answer: &PmAnswer) -> Result<(), String> {
        let (id, key) = (ruling.id, answer.choice);
        if answer.design {
            return Err("it flagged design".into());
        }
        // The settings file refuses `merge` as `true`, so this holds only a
        // merge a hand-edited state brings.
        let merge = matches!(ruling.kind, RulingKind::Merge { .. });
        if merge && self.settings.git.merging == Merging::Ask {
            return Err("a merge under `ask` is the maintainer's".into());
        }
        let picked = named(&ruling.kind, key);
        let note = format!("Answered by the project manager: {}", answer.why);
        let Some(ruled) = answer_for(&ruling.kind, key, &note) else {
            return Err(format!("option {key} is not one it lists"));
        };
        let by = Settler::Pm {
            answer: format!("{key}, {}", picked.unwrap_or_default()),
            why: answer.why.clone(),
            unread: false,
        };
        if let Err(e) = self.rule_by(id, ruled, by) {
            return Err(format!("its answer could not be applied: {e}"));
        }
        // An answer about a head that is gone withdraws the ruling instead.
        let withdrawn = (self.threads_told(id)).is_some_and(|t| t.starts_with("is withdrawn"));
        if !withdrawn {
            self.post_answered(&with_words(ruling, answer), key);
        }
        Ok(())
    }

    // Adds the project manager's pick and why to ruling `id`, for the card
    // the maintainer is asked with.
    fn pm_recommends(&mut self, id: TicketId, answer: &PmAnswer) {
        let mut next = self.state.clone();
        let Some(ruling) = next.rulings.iter_mut().find(|r| r.id == id) else {
            return;
        };
        *ruling = with_words(ruling, answer);
        if let Err(e) = self.save(next) {
            self.notes.push(format!(
                "cannot save the project manager's pick on ruling {id}: {e}"
            ));
        }
    }

    // Posts the answer on the ruling's thread, and, where the maintainer is
    // known, watches it for a `no <note>`.
    fn post_answered(&mut self, ruling: &Ruling, key: char) {
        let id = ruling.id;
        let name = named(&ruling.kind, key).unwrap_or_default();
        let line = format!(
            "**Answered by the project manager: {key}, {name}**. Nothing waits on the \
             maintainer. Reply `{id} no <note>` to reopen it."
        );
        let body = settled_comment(ruling, &line);
        let Some(thread) = ruling
            .pull_request
            .or_else(|| ruling.issue.and_then(|t| t.github()))
        else {
            return;
        };
        let failed = |e: &dyn std::fmt::Display| {
            format!("cannot post the project manager's answer on #{thread}: {e}")
        };
        if !self.app_covers() {
            if let Err(e) = self.comment_as_gh(thread, &body) {
                self.notes.push(failed(&e));
            }
            return;
        }
        let posted = match self.ports.voice.post(&self.remote, thread, &body) {
            Ok(posted) => posted,
            Err(e) => return self.notes.push(failed(&e)),
        };
        match self.maintainer() {
            Ok(login) => self.watch_decided(ruling.clone(), thread, &login, posted),
            Err(e) => self.notes.push(format!(
                "ruling {id}'s answer cannot be reopened from its thread, since the \
                 maintainer's login is not known: {e}"
            )),
        }
    }
}

// `ruling` with the project manager's pick and why under its question, as
// the card reads them.
fn with_words(ruling: &Ruling, answer: &PmAnswer) -> Ruling {
    let name = named(&ruling.kind, answer.choice).unwrap_or_else(|| answer.choice.to_string());
    let mut words = format!("{RECOMMENDS} {name}");
    if !answer.why.is_empty() {
        words.push_str(&format!("\n{SAYS} {}", answer.why));
    }
    Ruling {
        question: format!("{}\n\n{words}", ruling.question),
        ..ruling.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::ticket;

    #[test]
    fn the_project_managers_words_go_under_the_question() {
        let ruling = Ruling::new(
            ticket(3),
            "Rebase failed.",
            RulingKind::from(Stuck::Rebase {
                why: "conflicts".into(),
                review: None,
            }),
        );
        let answer = PmAnswer {
            choice: 'A',
            why: "main moved".into(),
            design: false,
        };

        let worded = with_words(&ruling, &answer);

        let name = named(&ruling.kind, 'A').expect("a rebase lists an option A");
        assert_eq!(
            worded.question,
            format!("Rebase failed.\n\nPM recommends: {name}\nPM says: main moved")
        );
    }
}
