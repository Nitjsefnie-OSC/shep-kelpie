//! A review bot's turn in the pass over a contribution's head
//!
//! A bot in `contributions.reviewers` answers on its own clock, so its turn
//! is no call: the runner steps it once a poll, with no slot held, and the
//! walk waits in the contribution's record ([`Walking`]) until the bot has
//! answered or been passed over. The turn is the work item's round, on the
//! same stages and the same profile: it takes the dog's lease, summons the
//! bot by its label or its comment, waits for the review, and reads the
//! bot's threads until two reads agree.
//!
//! A bot whose window opens far on, one that declines, one that does not
//! review in two hours, and one that cannot be summoned in two hours are
//! passed over, and the next reviewer reads. A bot's review of the head ends
//! the pass. Its open threads above a nit are the contribution's findings,
//! its nits kept apart, and kelpie posts nothing for it: the bot's own
//! comments are the post.

use super::super::Runner;
use super::super::review_bot::{
    ANSWER_WAIT, DONE_SETTLE, FAR, REVIEW_WAIT, lands, open_ids, read_threads, reread_due,
};
use crate::lease::wire::WindowFact;
use crate::ports::{Finding, Timestamp};
use crate::review_bot::{Activity, Bot, BotReviewer, Reading};
use crate::settings::{AgentName, ListedReviewer};
use crate::state::{BotTurn, Contribution, LeaseHeld, Pass, StateError, Walking};
use crate::work_item::{BotSkipped, ReviewStage};

// What a bot's turn came to at one step
enum Step {
    // Still waiting on the bot, its lease or its settling threads
    Waiting,
    // The pass goes on without the bot, for this reason
    Skipped(String),
    // The bot reviewed the head: its open threads above a nit, and its nits
    Answered {
        findings: Vec<Finding>,
        nits: Vec<Finding>,
    },
}

impl Runner {
    // Steps the walk of each green head whose next reviewer is a bot, once a
    // poll. A walk that comes to a reviewer that is not a bot waits for the
    // call that reads it, which the pass starts.
    pub(super) fn follow_bots(&mut self) -> Result<(), StateError> {
        self.release_idle_bots()?;
        if self.draining() {
            return Ok(());
        }
        let due: Vec<u64> = (self.state.contributions.iter())
            .filter(|c| c.is_passed() && c.is_green() && !c.is_reviewed())
            .map(|c| c.pull_request)
            .collect();
        for number in due {
            self.walk_bots(number)?;
        }
        Ok(())
    }

    // Walks `number`'s reviewers while the next is a bot: a bot that waits or
    // answers ends the step, and one passed over lets the walk go on.
    fn walk_bots(&mut self, number: u64) -> Result<(), StateError> {
        loop {
            let Some(held) = (self.state.contributions.iter()).find(|c| c.pull_request == number)
            else {
                return Ok(());
            };
            if held.is_reviewed() {
                return Ok(());
            }
            let Some(listed) = self.walk_at(held).cloned() else {
                let walked = held.walking_head().is_some();
                return if walked {
                    self.end_unread(number)
                } else {
                    Ok(())
                };
            };
            let Some(bot) = listed.bot() else {
                return Ok(());
            };
            match self.bot_turn(number, &listed, bot)? {
                Step::Waiting => return Ok(()),
                Step::Skipped(why) => {
                    let missed = format!("{} was passed over: {why}", listed.name);
                    self.go_past(number, &listed.name, missed)?;
                }
                Step::Answered { findings, nits } => {
                    return self.bot_answered(number, &listed.name, findings, nits);
                }
            }
        }
    }

    fn bot_turn(
        &mut self,
        number: u64,
        listed: &ListedReviewer,
        bot: BotReviewer,
    ) -> Result<Step, StateError> {
        let Some(held) = (self.state.contributions.iter()).find(|c| c.pull_request == number)
        else {
            return Ok(Step::Waiting);
        };
        let head = held.head.clone();
        let turn = held.walking_head().and_then(|w| w.bot.as_ref());
        let stage = match turn.filter(|t| t.reviewer == listed.name) {
            Some(turn) => turn.stage.clone(),
            None => {
                let stage = ReviewStage::Summon {
                    bot: bot.bot,
                    started: self.ports.clock.now(),
                    head,
                    readied: None,
                    full: false,
                };
                self.save_stage(number, &listed.name, stage.clone())?;
                stage
            }
        };
        let name = &listed.name;
        match stage {
            ReviewStage::Summon { started, head, .. } => {
                self.bot_summon(number, name, bot, started, head)
            }
            ReviewStage::Summoned {
                started, head, at, ..
            } => self.bot_await(number, name, bot, (started, head), at),
            ReviewStage::Settling {
                since, read, open, ..
            } => self.bot_settling(number, name, bot.bot, (since, read), &open),
            _ => unreachable!("a bot's turn only runs its summon"),
        }
    }

    // No summon without the lease, and none for a head already reviewed.
    fn bot_summon(
        &mut self,
        number: u64,
        reviewer: &AgentName,
        bot: BotReviewer,
        started: Timestamp,
        head: String,
    ) -> Result<Step, StateError> {
        let Some(activity) = self.bot_activity(number, bot.bot) else {
            return self.bot_unreadable(number, reviewer, bot.bot, started);
        };
        let profile = self.profile(bot.bot);
        let now = self.ports.clock.now();
        if profile.covers(&activity, &head) {
            self.release_bot(number, bot.bot)?;
            self.label_off(number, bot.bot);
            return self.start_settling(number, reviewer, bot.bot, &activity);
        }
        let opens = self.ports.leases.opens(&profile.lease(), bot.window, now);
        if let Some(opens) = opens.filter(|opens| opens.0 > now.0.saturating_add(FAR)) {
            self.release_bot(number, bot.bot)?;
            let skipped = BotSkipped::Window {
                reviewer: reviewer.clone(),
                opens,
            };
            return Ok(Step::Skipped(skipped.why()));
        }
        if now.0.saturating_sub(started.0) >= REVIEW_WAIT {
            self.release_bot(number, bot.bot)?;
            let since = started;
            let reviewer = reviewer.clone();
            return Ok(Step::Skipped(BotSkipped::Waited { reviewer, since }.why()));
        }
        if !self.bot_lease_granted(number, bot.bot) {
            return Ok(Step::Waiting);
        }
        match self.shut_out(bot.bot) {
            Ok(false) => {}
            Ok(true) => {
                self.release_bot(number, bot.bot)?;
                let reviewer = reviewer.clone();
                return Ok(Step::Skipped(BotSkipped::NotPublic { reviewer }.why()));
            }
            Err(reason) => {
                eprintln!("contribution #{number}: {reason}");
                return Ok(Step::Waiting);
            }
        }
        self.summon_bot(number, reviewer, bot.bot, (started, head), now)
    }

    // Sends the summon, under the lease, by the bot's label, or by its
    // comment when it has no label. The summon is saved before the comment
    // goes out, so a restart never posts it twice; a comment the forge
    // refused leaves the turn waiting to summon again.
    fn summon_bot(
        &mut self,
        number: u64,
        reviewer: &AgentName,
        bot: Bot,
        (started, head): (Timestamp, String),
        now: Timestamp,
    ) -> Result<Step, StateError> {
        let profile = self.profile(bot);
        self.hold_bot(bot, now)?;
        let by_comment = profile.label().is_none();
        let summoned = ReviewStage::Summoned {
            bot,
            started,
            head: head.clone(),
            at: now,
            full: by_comment,
            resent: false,
            failed: false,
        };
        // A label already on sends no event, so it comes off first.
        let labelled = self
            .put_label(bot, number, false)
            .and_then(|()| self.put_label(bot, number, true));
        if let Err(reason) = labelled {
            eprintln!("contribution #{number}: {reason}");
            self.release_bot(number, bot)?;
            return Ok(Step::Waiting);
        }
        self.save_stage(number, reviewer, summoned)?;
        self.note_ran(number, reviewer)?;
        if let Some(body) = profile.full_review().filter(|_| by_comment)
            && let Err(e) = self.comment_as_gh(number, body)
        {
            eprintln!(
                "contribution #{number}: cannot summon {}: {e}",
                profile.name()
            );
            let waiting = ReviewStage::Summon {
                bot,
                started,
                head,
                readied: None,
                full: false,
            };
            self.save_stage(number, reviewer, waiting)?;
            self.release_bot(number, bot)?;
        }
        Ok(Step::Waiting)
    }

    // Reads what the bot did since the summon made at `at`, and passes the
    // bot over when it refuses, declines, or stays silent for two hours.
    fn bot_await(
        &mut self,
        number: u64,
        reviewer: &AgentName,
        bot: BotReviewer,
        (started, head): (Timestamp, String),
        at: Timestamp,
    ) -> Result<Step, StateError> {
        let Some(activity) = self.bot_activity(number, bot.bot) else {
            return self.bot_unreadable(number, reviewer, bot.bot, started);
        };
        let profile = self.profile(bot.bot);
        let now = self.ports.clock.now().0;
        let waited = now.saturating_sub(at.0);
        let reviewer_name = reviewer.clone();
        match profile.read(&activity, &head, at) {
            Reading::Reviewed => self.bot_answered_at(number, reviewer, bot.bot, &activity, at),
            Reading::Completed { at: done } if now.saturating_sub(done.0) < DONE_SETTLE => {
                self.accepted_bot(number, bot.bot, at)?;
                Ok(Step::Waiting)
            }
            // The bot marked the head done with nothing posted: it found
            // nothing new in it, which is a clean read.
            Reading::Completed { .. } => {
                self.bot_answered_at(number, reviewer, bot.bot, &activity, at)
            }
            Reading::Refused { opens } => {
                let window = bot.window.seconds();
                let opens = opens.unwrap_or(Timestamp(now.saturating_add(window)));
                let kind = profile.lease();
                self.ports.leases.window(&kind, WindowFact::Opens, opens.0);
                self.release_bot(number, bot.bot)?;
                self.label_off(number, bot.bot);
                if opens.0 > now.saturating_add(FAR) {
                    let skipped = BotSkipped::Window {
                        reviewer: reviewer_name,
                        opens,
                    };
                    return Ok(Step::Skipped(skipped.why()));
                }
                let again = ReviewStage::Summon {
                    bot: bot.bot,
                    started,
                    head,
                    readied: None,
                    full: false,
                };
                self.save_stage(number, reviewer, again)?;
                Ok(Step::Waiting)
            }
            Reading::Declined { status } => {
                self.release_bot(number, bot.bot)?;
                self.label_off(number, bot.bot);
                let status = status.to_owned();
                let skipped = BotSkipped::Declined {
                    reviewer: reviewer_name,
                    status,
                };
                Ok(Step::Skipped(skipped.why()))
            }
            Reading::Silent | Reading::Processing | Reading::Failed if waited >= REVIEW_WAIT => {
                self.accepted_bot(number, bot.bot, at)?;
                self.label_off(number, bot.bot);
                let skipped = BotSkipped::Silent {
                    reviewer: reviewer_name,
                    head,
                };
                Ok(Step::Skipped(skipped.why()))
            }
            Reading::Processing | Reading::Failed => {
                self.accepted_bot(number, bot.bot, at)?;
                Ok(Step::Waiting)
            }
            // A summon neither taken up nor refused is counted as spent, so
            // the lease goes back.
            Reading::Silent if waited >= ANSWER_WAIT => {
                self.accepted_bot(number, bot.bot, at)?;
                Ok(Step::Waiting)
            }
            Reading::Silent => Ok(Step::Waiting),
        }
    }

    // The bot reviewed the head: its summon was answered, the lease and the
    // label go back, and its threads are read until they settle.
    fn bot_answered_at(
        &mut self,
        number: u64,
        reviewer: &AgentName,
        bot: Bot,
        activity: &Activity,
        at: Timestamp,
    ) -> Result<Step, StateError> {
        self.accepted_bot(number, bot, at)?;
        self.label_off(number, bot);
        self.start_settling(number, reviewer, bot, activity)
    }

    fn start_settling(
        &mut self,
        number: u64,
        reviewer: &AgentName,
        bot: Bot,
        activity: &Activity,
    ) -> Result<Step, StateError> {
        let now = self.ports.clock.now();
        let settling = ReviewStage::Settling {
            bot,
            since: now,
            read: now,
            open: open_ids(activity),
        };
        self.save_stage(number, reviewer, settling)?;
        self.note_ran(number, reviewer)?;
        Ok(Step::Waiting)
    }

    // Reads the threads again, and lands the review once two reads agree.
    fn bot_settling(
        &mut self,
        number: u64,
        reviewer: &AgentName,
        bot: Bot,
        (since, read): (Timestamp, Timestamp),
        open: &[String],
    ) -> Result<Step, StateError> {
        let now = self.ports.clock.now();
        if !reread_due(now, read) {
            return Ok(Step::Waiting);
        }
        let Some(activity) = self.bot_activity(number, bot) else {
            return self.bot_unreadable(number, reviewer, bot, since);
        };
        let threads = open_ids(&activity);
        if !lands(now, since, open, &threads) {
            let settling = ReviewStage::Settling {
                bot,
                since,
                read: now,
                open: threads,
            };
            self.save_stage(number, reviewer, settling)?;
            return Ok(Step::Waiting);
        }
        self.label_off(number, bot);
        let profile = self.profile(bot);
        let read = read_threads(profile.as_ref(), activity.open_threads());
        let apart = |threads: Vec<(String, Finding)>| threads.into_iter().map(|(_, f)| f).collect();
        Ok(Step::Answered {
            findings: apart(read.above),
            nits: apart(read.nits),
        })
    }

    // A turn whose bot cannot be read waits, until two hours since `since`
    // have passed, and then goes on without the bot, so a forge that stays
    // down cannot hold the pass.
    fn bot_unreadable(
        &mut self,
        number: u64,
        reviewer: &AgentName,
        bot: Bot,
        since: Timestamp,
    ) -> Result<Step, StateError> {
        let now = self.ports.clock.now();
        if now.0.saturating_sub(since.0) < REVIEW_WAIT {
            return Ok(Step::Waiting);
        }
        self.release_bot(number, bot)?;
        self.label_off(number, bot);
        let reviewer = reviewer.clone();
        Ok(Step::Skipped(BotSkipped::Waited { reviewer, since }.why()))
    }

    // Reads `bot`'s activity on `number`, or logs why it cannot and leaves the
    // turn for the next poll.
    fn bot_activity(&self, number: u64, bot: Bot) -> Option<Activity> {
        match self.activity(bot, number) {
            Ok(activity) => Some(activity),
            Err(reason) => {
                eprintln!("contribution #{number}: {reason}");
                None
            }
        }
    }

    // Takes `bot`'s label off, if it has one. A label left on would summon the
    // bot on the next push, outside its lease, so a failure is logged for the
    // next pass to find on.
    fn label_off(&self, number: u64, bot: Bot) {
        if let Err(reason) = self.put_label(bot, number, false) {
            eprintln!("contribution #{number}: {reason}");
        }
    }

    // Records the pass the bot's review ended: its threads above a nit as the
    // findings, its nits apart, and the bot as the reviewer the next pass
    // starts after.
    fn bot_answered(
        &mut self,
        number: u64,
        reviewer: &AgentName,
        findings: Vec<Finding>,
        nits: Vec<Finding>,
    ) -> Result<(), StateError> {
        let mut next = self.state.clone();
        let Some(held) = (next.contributions.iter_mut()).find(|c| c.pull_request == number) else {
            return Ok(());
        };
        let mut ran = held.walking_head().map_or_else(Vec::new, |w| w.ran.clone());
        if !ran.contains(reviewer) {
            ran.push(reviewer.clone());
        }
        held.pass = Some(Pass {
            head: held.head.clone(),
            ran,
            answered: Some(reviewer.clone()),
            unread: None,
            findings,
            nits,
            review: None,
            note: None,
        });
        held.last_answered = Some(reviewer.clone());
        held.walking = None;
        self.save(next)
    }

    // Ends the pass unread: every reviewer the walk was past passed.
    fn end_unread(&mut self, number: u64) -> Result<(), StateError> {
        let mut next = self.state.clone();
        let Some(held) = (next.contributions.iter_mut()).find(|c| c.pull_request == number) else {
            return Ok(());
        };
        let walking = held.walking.take().filter(|w| w.head == held.head);
        let (ran, missed) = walking.map_or_else(Default::default, |w| (w.ran, w.missed));
        let unread = match missed.is_empty() {
            true => "no reviewer is listed".to_owned(),
            false => missed.join(", "),
        };
        held.pass = Some(Pass {
            head: held.head.clone(),
            ran,
            answered: None,
            unread: Some(unread),
            findings: Vec::new(),
            nits: Vec::new(),
            review: None,
            note: None,
        });
        self.save(next)
    }

    // Records the stage of `reviewer`'s turn, starting the walk's record
    // where it has none.
    fn save_stage(
        &mut self,
        number: u64,
        reviewer: &AgentName,
        stage: ReviewStage,
    ) -> Result<(), StateError> {
        let reviewer = reviewer.clone();
        self.save_walking(number, |w| w.bot = Some(BotTurn { reviewer, stage }))
    }

    // Notes that `reviewer` was called in the walk
    fn note_ran(&mut self, number: u64, reviewer: &AgentName) -> Result<(), StateError> {
        let reviewer = reviewer.clone();
        self.save_walking(number, |w| {
            if !w.ran.contains(&reviewer) {
                w.ran.push(reviewer);
            }
        })
    }

    // Goes past `reviewer`, which read nothing, for the reason `missed`.
    fn go_past(
        &mut self,
        number: u64,
        reviewer: &AgentName,
        missed: String,
    ) -> Result<(), StateError> {
        let reviewer = reviewer.clone();
        self.save_walking(number, |w| {
            w.past.push(reviewer);
            w.missed.push(missed);
            w.bot = None;
        })
    }

    fn save_walking(
        &mut self,
        number: u64,
        change: impl FnOnce(&mut Walking),
    ) -> Result<(), StateError> {
        let mut next = self.state.clone();
        let Some(held) = (next.contributions.iter_mut()).find(|c| c.pull_request == number) else {
            return Ok(());
        };
        let head = held.head.clone();
        let walking = held.walking.take().filter(|w| w.head == head);
        let mut walking = walking.unwrap_or_else(|| Walking {
            head,
            past: Vec::new(),
            ran: Vec::new(),
            missed: Vec::new(),
            bot: None,
        });
        change(&mut walking);
        held.walking = Some(walking);
        self.save(next)
    }

    // Asks for `bot`'s lease for the turn on `number`, and says whether the
    // dog granted it and no one else's turn holds it: a grant that a work
    // item or another contribution holds is not this turn's to use.
    fn bot_lease_granted(&self, number: u64, bot: Bot) -> bool {
        let lease = self.profile(bot).lease();
        self.ports.leases.want(&lease);
        let elsewhere = self
            .state
            .leases
            .iter()
            .any(|l| l.resource == bot.resource() && l.issue.is_some())
            || (self.state.contributions.iter())
                .any(|c| c.pull_request != number && summoned_on(c) == Some(bot));
        self.ports.leases.holds(&lease) && !elsewhere
    }

    // Records that the turn took `bot`'s lease, which no work item's row holds
    fn hold_bot(&mut self, bot: Bot, since: Timestamp) -> Result<(), StateError> {
        let mut next = self.state.clone();
        next.leases.retain(|l| l.resource != bot.resource());
        next.leases.push(LeaseHeld {
            resource: bot.resource(),
            issue: None,
            since,
        });
        self.save(next)
    }

    // What a summon's answer tells the dog: the window runs from `at`.
    fn accepted_bot(&mut self, number: u64, bot: Bot, at: Timestamp) -> Result<(), StateError> {
        let kind = self.profile(bot).lease();
        self.ports.leases.window(&kind, WindowFact::Summoned, at.0);
        self.release_bot(number, bot)
    }

    // Gives `bot`'s lease back for the turn on `number`: when the turn holds
    // it, or when no work item holds it or waits for it. Another contribution
    // whose turn is summoned on the bot holds the lease row, so it is left
    // alone: its row has no issue, as this turn's would not.
    fn release_bot(&mut self, number: u64, bot: Bot) -> Result<(), StateError> {
        let theirs = (self.state.contributions.iter())
            .any(|c| c.pull_request != number && summoned_on(c) == Some(bot));
        if theirs {
            return Ok(());
        }
        self.give_back_bot(bot, Some(number))
    }

    // Gives the lease back, as `release_bot` does once it is no other turn's.
    // The ask and the grant are the project's, so giving them back for a turn
    // that never took the lease would cancel another round's place in the
    // dog's queue: a work item's, or a contribution's other than `except`.
    fn give_back_bot(&mut self, bot: Bot, except: Option<u64>) -> Result<(), StateError> {
        let mine = |l: &LeaseHeld| l.resource == bot.resource() && l.issue.is_none();
        let held = self.state.leases.iter().any(mine);
        if !held && (self.bot_held_or_wanted_by_an_item(bot) || self.turn_waits_on(bot, except)) {
            return Ok(());
        }
        self.ports.leases.give_back(&self.profile(bot).lease());
        if !held {
            return Ok(());
        }
        let mut next = self.state.clone();
        next.leases.retain(|l| !mine(l));
        self.save(next)
    }

    // Whether a contribution other than `except` has a turn waiting to summon `bot`
    fn turn_waits_on(&self, bot: Bot, except: Option<u64>) -> bool {
        (self.state.contributions.iter())
            .any(|c| Some(c.pull_request) != except && waiting_on(c) == Some(bot))
    }

    fn bot_held_or_wanted_by_an_item(&self, bot: Bot) -> bool {
        let held =
            (self.state.leases.iter()).any(|l| l.resource == bot.resource() && l.issue.is_some());
        held || self.item_waits_on(bot, None)
    }

    // Gives back the lease of every bot that no turn is summoned on any more,
    // as after a push cleared the walk that held it.
    pub(super) fn release_idle_bots(&mut self) -> Result<(), StateError> {
        let idle: Vec<Bot> = (self.state.leases.iter())
            .filter(|l| l.issue.is_none())
            .filter_map(|l| Bot::of(l.resource))
            .filter(|bot| (self.state.contributions.iter()).all(|c| summoned_on(c) != Some(*bot)))
            .collect();
        for bot in idle {
            self.give_back_bot(bot, None)?;
        }
        Ok(())
    }

    // Gives back the ask of each bot in `asked`, whose turns waited to summon
    // it and were cleared, unless a turn still summoned or waiting on it
    // needs it. Called once the walks are cleared.
    pub(super) fn give_up_asks(&mut self, asked: &[Bot]) -> Result<(), StateError> {
        for bot in asked {
            let summoned = (self.state.contributions.iter()).any(|c| summoned_on(c) == Some(*bot));
            if !summoned {
                self.give_back_bot(*bot, None)?;
            }
        }
        Ok(())
    }

    // Takes the label off each pull request whose walk is about to be cleared
    // while its bot is summoned, because the contribution is no longer
    // followed or has a new head. A push with the label on would summon the
    // bot outside its lease. Returns the bots whose turns were still waiting
    // to summon, which may have asked for the lease.
    #[must_use]
    pub(super) fn abandon_turns(&self, followed: &[Contribution]) -> Vec<Bot> {
        let mut asked = Vec::new();
        for held in &self.state.contributions {
            let moved = followed
                .iter()
                .all(|c| c.pull_request != held.pull_request || c.head != held.head);
            if !moved {
                continue;
            }
            if let Some(bot) = labelled_on(held) {
                self.label_off(held.pull_request, bot);
            }
            if let Some(bot) = waiting_on(held)
                && !asked.contains(&bot)
            {
                asked.push(bot);
            }
        }
        asked
    }
}

impl Runner {
    // Ends the bot turns of reviewers `lineup` no longer lists, so a label
    // they put on a pull request comes off and their lease goes back before
    // the walk moves on and forgets them. A turn of a reviewer still listed
    // goes on.
    pub(in crate::runner) fn end_unlisted_turns(
        &mut self,
        lineup: &[ListedReviewer],
    ) -> Result<(), StateError> {
        let listed = |name: &AgentName| lineup.iter().any(|r| &r.name == name && r.bot().is_some());
        let unlisted = |c: &Contribution| {
            let turn = c.walking_head().and_then(|w| w.bot.as_ref());
            turn.is_some_and(|t| !listed(&t.reviewer))
        };
        let ended: Vec<&Contribution> = (self.state.contributions.iter())
            .filter(|c| unlisted(c))
            .collect();
        if ended.is_empty() {
            return Ok(());
        }
        let mut asked = Vec::new();
        let mut labelled = Vec::new();
        for held in &ended {
            labelled.extend(labelled_on(held).map(|bot| (held.pull_request, bot)));
            asked.extend(waiting_on(held));
        }
        let mut next = self.state.clone();
        for held in next.contributions.iter_mut().filter(|c| unlisted(c)) {
            if let Some(walking) = held.walking.as_mut() {
                walking.bot = None;
            }
        }
        self.save(next)?;
        // After the save, so a turn that stays in the state keeps its label.
        for (number, bot) in labelled {
            self.label_off(number, bot);
        }
        self.give_up_asks(&asked)?;
        self.release_idle_bots()
    }
}

// The bot `contribution`'s walk has summoned and has not yet landed the
// review of, so its label may still be on
fn labelled_on(contribution: &Contribution) -> Option<Bot> {
    let turn = contribution.walking_head()?.bot.as_ref()?;
    match turn.stage {
        ReviewStage::Summoned { bot, .. } | ReviewStage::Settling { bot, .. } => Some(bot),
        _ => None,
    }
}

// The bot `contribution`'s walk has summoned and holds the lease for, until
// the bot answers
fn summoned_on(contribution: &Contribution) -> Option<Bot> {
    let turn = contribution.walking_head()?.bot.as_ref()?;
    match turn.stage {
        ReviewStage::Summoned { bot, .. } => Some(bot),
        _ => None,
    }
}

// The bot `contribution`'s walk waits to summon
pub(in crate::runner) fn waiting_on(contribution: &Contribution) -> Option<Bot> {
    let turn = contribution.walking_head()?.bot.as_ref()?;
    match turn.stage {
        ReviewStage::Summon { bot, .. } => Some(bot),
        _ => None,
    }
}
