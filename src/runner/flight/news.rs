//! What a call's thread sends back, and what it needs to send it
//!
//! One channel carries every kind of news. A call's thread holds a [`Tell`],
//! which sends on it and wakes the runner's loop, and waits until the runner
//! has heard what it sent when the runner must know it before the call goes
//! on. A call that could not start ends the same way, as [`News::Ended`].

use std::any::Any;
use std::sync::mpsc::{self, Sender};

use crate::ports::{AgentError, AgentReply, RoundStage, Wait};
use crate::runner::report::{ReviewResult, Reviewed};
use crate::ticket::TicketId;

// What a call's thread sends the runner
pub(super) enum News {
    Ended {
        ticket: TicketId,
        end: End,
    },
    // The project manager's call ended
    Pm(End),
    // The batch retro's call ended
    Retro(End),
    // The issue writer's call labelling an issue ended
    Labelled {
        issue: u64,
        end: End,
    },
    // The e2e run of a bump's head ended
    Proved(super::proving::Proof),
    // A contribution's head was read for security, to a verdict or to none
    Secured {
        pull_request: u64,
        head: String,
        reading: crate::runner::contributions::Reading,
    },
    // The triager's research of an issue ended, with its reply read against its
    // view. The reply is boxed: it is the largest thing any `News` variant holds
    // by far, and the rest of the enum is small next to it.
    Triaged {
        issue: u64,
        end: End,
        found: Option<Box<Result<crate::runner::triage::research::Found, String>>>,
    },
    // The experts of a ruling ended, each as its own call did
    Paneled {
        id: u64,
        ends: Vec<(crate::panel::Voter, Result<AgentReply, AgentError>)>,
    },
    // A local round queued for the GPU or began to run. The call waits until
    // `_seen` drops, once the stage is saved, so its time is charged right.
    Stage {
        ticket: TicketId,
        stage: RoundStage,
        _seen: Sender<()>,
    },
    // The call has the lease it waited for, and begins now. It waits until
    // `_seen` drops, once its ceiling counts from now.
    Began {
        ticket: TicketId,
        _seen: Sender<()>,
    },
    // The call waits for its model
    Waits {
        ticket: TicketId,
        wait: Wait,
    },
}

// How a call ended
pub(super) enum End {
    Turn(Result<AgentReply, AgentError>),
    Review(Reviewed),
    // The call's thread panicked, which ends the runner as a panic in its
    // own thread does
    Panicked(Box<dyn Any + Send>),
}

impl End {
    // Whether it ended before reaching a model: its settings, its harness or
    // its session never got that far
    pub(super) fn reached_no_model(&self) -> bool {
        match self {
            Self::Turn(result) => crate::usage::ended(result).is_none(),
            Self::Review(reviewed) => matches!(
                (&reviewed.result, &reviewed.spent),
                (ReviewResult::Findings(Err(_)), None)
            ),
            Self::Panicked(_) => false,
        }
    }
}

// What a call's thread needs to tell the runner about its call
#[derive(Clone)]
pub(super) struct Tell {
    pub(super) ticket: TicketId,
    pub(super) news: Sender<News>,
    pub(super) wake: Option<Sender<()>>,
}

impl Tell {
    pub(super) fn send(&self, news: News) -> bool {
        let sent = self.news.send(news).is_ok();
        if let Some(wake) = &self.wake {
            let _ = wake.send(());
        }
        sent
    }

    pub(super) fn stage(&self, stage: RoundStage) {
        let ticket = self.ticket;
        self.until_heard(|seen| News::Stage {
            ticket,
            stage,
            _seen: seen,
        });
    }

    pub(super) fn began(&self) {
        let ticket = self.ticket;
        self.until_heard(|seen| News::Began {
            ticket,
            _seen: seen,
        });
    }

    pub(super) fn waits(&self, wait: Wait) {
        let ticket = self.ticket;
        self.send(News::Waits { ticket, wait });
    }

    // Sends the news `news` makes, and waits until the runner has heard it.
    fn until_heard(&self, news: impl FnOnce(Sender<()>) -> News) {
        let (seen, heard) = mpsc::channel();
        if self.send(news(seen)) {
            let _ = heard.recv();
        }
    }
}
