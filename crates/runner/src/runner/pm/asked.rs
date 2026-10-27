//! The rulings the maintainer puts to the project manager
//!
//! A ruling that waits on the maintainer wakes no project manager of itself.
//! A reply in its thread asks: `<id> pm recommend` for its pick, posted
//! there while the ruling stays the maintainer's, and `<id> pm options` for
//! other approaches, kept on the ruling. `<id> pm <n>` takes approach n, as
//! a `no <note>` or a question's answer. A wake that fails or gives no
//! answer says so in the thread.

use serde_json::{Map, Value};

use super::Wake;
use super::act::{WORDS, plain};
use crate::pm_answer::{PmAnswer, WHY_LONGEST, from_fields};
use crate::ports::ThreadComment;
use crate::runner::voice::{answer_for, brief, inert, named};
use crate::runner::words::Wants;
use crate::runner::{Answer, Runner};
use crate::state::Ruling;
use crate::ticket::TicketId;

/// What a `pm` reply takes after `pm`
const TAKES: &str = "`pm` takes `recommend`, `options` or an approach's number";

/// The most approaches a ruling keeps
const APPROACHES: usize = 5;

/// What the maintainer asks of the project manager about a ruling
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::runner) enum Asked {
    /// Its pick and why, posted in the thread, the ruling still theirs
    Recommend,
    /// Other approaches, for when none of its options is right
    Options,
}

/// A reply to a ruling that starts with `pm`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::runner) enum PmReply {
    /// Asks the project manager
    Ask(Asked),
    /// Takes the approach of this number, 1 for the first
    Take(usize),
}

/// The `pm` reply `words` are to the ruling read by `number`, if they
/// start with `pm`, or with `number` and then `pm`
///
/// # Errors
///
/// What a `pm` reply takes, when the rest is none of them.
pub(in crate::runner) fn pm_reply(
    words: &str,
    number: TicketId,
) -> Option<Result<PmReply, String>> {
    let mut parts = words.split_whitespace().peekable();
    let numbered =
        |first: &&str| first.trim_start_matches('#').parse::<TicketId>().ok() == Some(number);
    if parts.peek().is_some_and(numbered) {
        parts.next();
    }
    if !parts.next()?.eq_ignore_ascii_case("pm") {
        return None;
    }
    let word = parts.next().map(str::to_ascii_lowercase);
    Some(match (word.as_deref(), parts.next()) {
        (Some("recommend"), None) => Ok(PmReply::Ask(Asked::Recommend)),
        (Some("options"), None) => Ok(PmReply::Ask(Asked::Options)),
        (Some(n), None) => match n.parse::<usize>() {
            Ok(n) if n > 0 => Ok(PmReply::Take(n)),
            _ => Err(TAKES.to_owned()),
        },
        _ => Err(TAKES.to_owned()),
    })
}

/// What a wake for `asked` on ruling `id` tells the project manager,
/// before the ruling itself
pub(super) fn lead(asked: Asked, id: TicketId) -> String {
    match asked {
        Asked::Recommend => format!(
            "the maintainer asks which option you recommend for ruling {id}. Give it as your \
             vote on it in `votes`: kelpie posts it in the ruling's thread, and the maintainer \
             still answers the ruling:\n"
        ),
        Asked::Options => format!(
            "the maintainer finds none of ruling {id}'s options right and asks you for other \
             approaches. Give them as your vote on it in `votes`, `{{\"ruling\": {id}, \
             \"approaches\": [\"<what the worker should do instead, one line of at most 350 \
             characters>\"]}}`, at most {APPROACHES}: kelpie posts them in the ruling's thread, \
             and the one the maintainer takes goes to the worker as their note, or as their \
             answer to a question:\n"
        ),
    }
}

impl Runner {
    /// Puts ruling `id` to the project manager for `asked`, as `comment`
    /// asks, reacting if it took and saying why not in its thread
    pub(in crate::runner) fn pm_asked_by(
        &mut self,
        comment: &ThreadComment,
        id: TicketId,
        asked: Asked,
    ) {
        let (who, thread) = (&comment.author, comment.thread);
        match self.pm_ask(id, asked) {
            Ok(()) => {
                if let Err(e) = self.ports.voice.thumbs_up(&self.remote, comment.id) {
                    self.notes
                        .push(format!("cannot react to {who}'s request on #{thread}: {e}"));
                }
            }
            Err(why) => {
                let number = self.ruling_number(id);
                let said =
                    format!("@{who} Ruling #{number} was not put to the project manager: {why}.");
                self.say(thread, &said);
            }
        }
    }

    // Queues the wake that asks the project manager, once at a time.
    fn pm_ask(&mut self, id: TicketId, asked: Asked) -> Result<(), String> {
        if self.agents.pm.is_none() {
            return Err("this project names no project manager".into());
        }
        let ruling = (self.state.rulings.iter().find(|r| r.id == id))
            .ok_or_else(|| "it no longer waits".to_owned())?;
        if self.pm_has_asked(id) {
            return Err("the project manager is already on it, and answers in this thread".into());
        }
        let text = brief(ruling).0;
        self.pm.asking.insert(id, self.ports.clock.now());
        self.pm_due(Wake::Answer {
            id,
            asked: text,
            request: Some(asked),
        });
        Ok(())
    }

    /// Acts on the project manager's `votes` on the rulings in `requested`,
    /// which the maintainer asked about, and says what each did, for the log
    pub(super) fn pm_requested(
        &mut self,
        requested: &[(TicketId, Asked)],
        votes: &[(TicketId, Map<String, Value>)],
    ) -> Vec<String> {
        let mut done = Vec::new();
        for &(id, asked) in requested {
            self.pm.asking.remove(&id);
            let Some(ruling) = self.state.rulings.iter().find(|r| r.id == id).cloned() else {
                done.push(format!("left ruling {id}: it was answered meanwhile"));
                continue;
            };
            let fields = (votes.iter().rev()).find(|(vote, _)| *vote == id);
            let acted = match (fields, asked) {
                (Some((_, fields)), Asked::Recommend) => self.pm_recommend(&ruling, fields),
                (Some((_, fields)), Asked::Options) => self.pm_options(&ruling, fields),
                (None, _) => Err("its reply held no answer to it".to_owned()),
            };
            match acted {
                Ok(line) => done.push(line),
                Err(why) => {
                    self.pm_did_not_answer(&ruling);
                    done.push(format!("gave no answer to ruling {id}: {why}"));
                }
            }
        }
        done
    }

    // Posts the project manager's pick for `ruling` in its thread.
    fn pm_recommend(
        &mut self,
        ruling: &Ruling,
        fields: &Map<String, Value>,
    ) -> Result<String, String> {
        let answer = self.pm_vote(ruling, fields)?;
        self.pm_recommends(ruling.id, &answer);
        self.post_recommended(ruling, &answer, None);
        let (choice, id) = (answer.choice, ruling.id);
        Ok(format!("recommended option {choice} on ruling {id}"))
    }

    // Keeps the approaches the project manager gave for `ruling` on it, and
    // posts them in its thread, inert, with how to take one.
    fn pm_options(
        &mut self,
        ruling: &Ruling,
        fields: &Map<String, Value>,
    ) -> Result<String, String> {
        let Some(Value::Array(items)) = fields.get("approaches") else {
            return Err("`approaches` is not a list".into());
        };
        let approaches: Vec<String> = (items.iter())
            .filter_map(Value::as_str)
            .map(|text| plain(&self.shown(text.to_owned()), WHY_LONGEST))
            .filter(|text| !text.is_empty())
            .take(APPROACHES)
            .collect();
        if approaches.is_empty() {
            return Err("it gave no approaches".into());
        }
        let id = ruling.id;
        let mut next = self.state.clone();
        if let Some(kept) = next.rulings.iter_mut().find(|r| r.id == id) {
            kept.approaches.clone_from(&approaches);
        }
        self.save(next)
            .map_err(|e| format!("cannot save its approaches: {e}"))?;
        let number = ruling.number();
        let listed: Vec<String> = (approaches.iter().zip(1..))
            .map(|(text, n)| format!("{n}. {}", inert(text)))
            .collect();
        let taken = match Wants::of(&ruling.kind) {
            Wants::Answer => "as your answer".to_owned(),
            Wants::Merge | Wants::YesOrNo => format!("as your `{number} no <note>`"),
        };
        let said = format!(
            "**Project manager: other approaches to ruling #{number}**\n\n{}\n\nReply \
             `{number} pm <n>` to take one {taken}.",
            listed.join("\n")
        );
        self.pm_says(ruling, &said);
        let count = approaches.len();
        Ok(format!("gave ruling {id} {count} other approaches"))
    }

    /// The answer approach `n` of ruling `id` gives: the maintainer's note,
    /// or a question's answer
    ///
    /// # Errors
    ///
    /// Why approach `n` is none, for the reply.
    pub(in crate::runner) fn approach(&self, id: TicketId, n: usize) -> Result<Answer, String> {
        let ruling = (self.state.rulings.iter().find(|r| r.id == id))
            .ok_or_else(|| "it no longer waits".to_owned())?;
        let number = ruling.number();
        let Some(text) = n.checked_sub(1).and_then(|at| ruling.approaches.get(at)) else {
            return Err(match ruling.approaches.len() {
                0 => format!(
                    "the project manager gave it no approaches: reply `{number} pm options` for \
                     some"
                ),
                count => format!("it has {count} approaches, so `pm {n}` names none"),
            });
        };
        Ok(match Wants::of(&ruling.kind) {
            Wants::Answer => Answer::Text(text.clone()),
            Wants::Merge | Wants::YesOrNo => Answer::No(text.clone()),
        })
    }

    // The project manager's answer to `ruling` in `fields`, its why cut to
    // what a ruling carries.
    fn pm_vote(&self, ruling: &Ruling, fields: &Map<String, Value>) -> Result<PmAnswer, String> {
        let answer = from_fields(fields, brief(ruling).1)?;
        let why = plain(&self.shown(answer.why.clone()), WORDS);
        Ok(PmAnswer { why, ..answer })
    }

    // Posts the project manager's pick for `ruling` in its thread, where the
    // maintainer still answers it, saying why it is theirs where `since` does.
    fn post_recommended(&mut self, ruling: &Ruling, answer: &PmAnswer, since: Option<&str>) {
        let (number, key) = (ruling.number(), answer.choice);
        let mut blocks = vec![match answer.why.is_empty() {
            true => format!("**Project manager: {key}**"),
            false => format!("**Project manager: {key}**. {}", inert(&answer.why)),
        }];
        blocks.extend(reply_form(ruling, key).map(|form| format!("It would answer {form}.")));
        blocks.push(match since {
            Some(why) => format!(
                "Ruling #{number} is still yours, since {why}: answer it as its comment says."
            ),
            None => format!("Ruling #{number} is still yours: answer it as its comment says."),
        });
        self.pm_says(ruling, &blocks.join("\n\n"));
    }

    /// Says in `ruling`'s thread that the project manager did not answer
    /// what the maintainer asked of it
    pub(super) fn pm_did_not_answer(&mut self, ruling: &Ruling) {
        let number = ruling.number();
        let said = format!(
            "The project manager did not answer what you asked of ruling #{number}, which is \
             still yours."
        );
        self.pm_says(ruling, &said);
    }

    // Posts `text` in `ruling`'s thread, to the maintainer where they are known.
    fn pm_says(&mut self, ruling: &Ruling, text: &str) {
        let Some(thread) = (ruling.pull_request).or_else(|| ruling.issue.and_then(|t| t.github()))
        else {
            return;
        };
        let text = match self.maintainer() {
            Ok(login) => format!("@{login} {text}"),
            Err(_) => text.to_owned(),
        };
        self.say(thread, &text);
    }
}

// How the maintainer answers `ruling` with option `key`, in its thread.
fn reply_form(ruling: &Ruling, key: char) -> Option<String> {
    let name = inert(&named(&ruling.kind, key)?);
    Some(match answer_for(&ruling.kind, key, "<note>")? {
        Answer::Text(_) => format!("with the option {name}"),
        answer => format!("`{}` ({name})", answer.params(ruling.number())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::ticket;

    #[test]
    fn a_pm_reply_is_read_with_or_without_the_rulings_number() {
        let read = |words| pm_reply(words, ticket(71));
        assert_eq!(
            read("pm recommend"),
            Some(Ok(PmReply::Ask(Asked::Recommend)))
        );
        assert_eq!(
            read("71 PM Recommend"),
            Some(Ok(PmReply::Ask(Asked::Recommend)))
        );
        assert_eq!(read("pm options"), Some(Ok(PmReply::Ask(Asked::Options))));
        assert_eq!(read("71 pm 2"), Some(Ok(PmReply::Take(2))));
        assert_eq!(read("pm 0"), Some(Err(TAKES.to_owned())));
        assert_eq!(read("pm"), Some(Err(TAKES.to_owned())));
        assert_eq!(read("pm recommend now"), Some(Err(TAKES.to_owned())));
        assert_eq!(read("yes"), None);
        assert_eq!(read("72 pm recommend"), None);
    }
}
