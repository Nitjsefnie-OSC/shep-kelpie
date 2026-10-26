//! Posting a ruling's comment on its thread
//!
//! [`Runner::ruling_post`] settles under the lock whether a ruling's comment goes up and where,
//! [`PostJob::send`] posts it, and [`Runner::ruling_posted`] keeps what came of it. The posts of
//! rulings a hold has let go of send with the runner's lock let go; the others, made as a ruling is
//! raised, send in [`Runner::post_ruling`] with it held.

use std::sync::Arc;

use super::comment;
use crate::ports::{Forge, ForgeError, Tickets, Voice};
use crate::runner::Runner;
use crate::runner::voice::{Placed, post_app_ruling};
use crate::settings::ForgeSlug;
use crate::state::Ruling;
use crate::ticket::TicketId;

/// A ruling's comment, and where it goes
enum Post {
    /// On a local ticket, which no App or `gh` reaches
    Ticket { ticket: TicketId, comment: String },
    /// Through the App, mentioning the maintainer when `maintainer` is known, or else the login
    /// `gh` acts as
    App {
        thread: u64,
        ruling: Box<Ruling>,
        maintainer: Option<String>,
    },
    /// As the login `gh` acts as
    Gh { number: u64, comment: String },
}

/// A ruling's comment ready to post, which needs no lock
pub(in crate::runner) struct PostJob {
    id: TicketId,
    post: Post,
    remote: ForgeSlug,
    forge: Arc<dyn Forge>,
    tickets: Arc<dyn Tickets>,
    voice: Arc<dyn Voice>,
}

/// What came of a post
pub(in crate::runner) struct Sent {
    id: TicketId,
    /// The thread an App's comment went to
    thread: Option<u64>,
    /// Why the post failed, if it did
    failed: Option<String>,
    /// The pull request or issue and id of the comment posted as the login `gh` acts as
    comment: Option<(u64, u64)>,
    /// The login `gh` acts as, where the forge was asked
    viewer: Option<String>,
    /// Where an App's comment is kept
    placed: Option<Placed>,
    /// Why an App's comment went up without a mention
    unasked: Option<ForgeError>,
}

impl PostJob {
    /// The ruling it posts
    pub(in crate::runner) fn id(&self) -> TicketId {
        self.id
    }

    /// Posts the comment and says what came of it
    pub(in crate::runner) fn send(self) -> Sent {
        let mut sent = Sent {
            id: self.id,
            thread: None,
            failed: None,
            comment: None,
            viewer: None,
            placed: None,
            unasked: None,
        };
        match &self.post {
            Post::Ticket { ticket, comment } => {
                let posted = self.tickets.comment_ticket(&self.remote, *ticket, comment);
                sent.failed = posted.err().map(|e| e.to_string());
            }
            Post::Gh { number, comment } => {
                match self.forge.comment(&self.remote, *number, comment) {
                    Ok(id) => sent.comment = Some((*number, id)),
                    Err(e) => sent.failed = Some(e.to_string()),
                }
            }
            Post::App {
                thread,
                ruling,
                maintainer,
            } => {
                let mention = match maintainer {
                    Some(login) => Ok(login.clone()),
                    None => self.forge.viewer().inspect(|login| {
                        sent.viewer = Some(login.clone());
                    }),
                };
                let app = post_app_ruling(&*self.voice, &self.remote, *thread, ruling, mention);
                sent.thread = Some(*thread);
                sent.failed = app.failed;
                sent.placed = app.placed;
                sent.unasked = app.unasked;
            }
        }
        sent
    }
}

impl Runner {
    // Posts a saved ruling's comment with the lock held, and returns why it failed, if it did
    pub(in crate::runner) fn post_ruling(
        &mut self,
        number: Option<u64>,
        id: TicketId,
    ) -> Option<String> {
        let sent = self.ruling_post(number, id)?.send();
        self.ruling_posted(sent)
    }

    /// The comment of a saved ruling on its pull request, if it has one and has anything to say
    ///
    /// The pull request carries no question: the webhook does. Where an App covers the repo, the
    /// ruling is posted through it on the pull request, or on the issue while there is none,
    /// and mentions the maintainer. Its answer is saved with the login it mentions and the
    /// comment's own id and time as GitHub gave them.
    pub(in crate::runner) fn ruling_post(
        &mut self,
        number: Option<u64>,
        id: TicketId,
    ) -> Option<PostJob> {
        // A contribution's ruling stays off the contributor's pull request.
        if (self.state.rulings.iter()).any(|r| r.id == id && r.kind.is_contribution()) {
            return None;
        }
        // A question the project manager may answer, or a ruling the panel has
        // yet to vote on, is not the maintainer's yet, so its comment waits
        // for the hold to end.
        if self.panel_enabled() {
            self.mark_unposted(id, true);
        }
        self.panel_open_all();
        if self.pm_holds_comment(id) {
            return None;
        }
        self.panel_unvoted(id);
        self.mark_unposted(id, false);
        let ruling = self.state.rulings.iter().find(|r| r.id == id)?;
        let post = if number.is_none()
            && let Some(ticket) = ruling.issue.filter(|t| t.github().is_none())
        {
            // Before there is a pull request, a local ticket's ruling goes on the
            // ticket: a GitHub thread is the repo's, which a local ticket keeps clear.
            Post::Ticket {
                ticket,
                comment: comment(&ruling.kind)?,
            }
        } else if self.app_covers() {
            // Only a GitHub ticket is a thread to comment on.
            let thread = number.or(ruling.issue.and_then(|ticket| ticket.github()))?;
            Post::App {
                thread,
                ruling: Box::new(ruling.clone()),
                maintainer: self.known_maintainer(),
            }
        } else {
            Post::Gh {
                number: number?,
                comment: comment(&ruling.kind)?,
            }
        };
        Some(PostJob {
            id,
            post,
            remote: self.remote.clone(),
            forge: Arc::clone(&self.ports.forge),
            tickets: Arc::clone(&self.ports.tickets),
            voice: Arc::clone(&self.ports.voice),
        })
    }

    // Keeps what came of a post, and returns why its comment failed, if it did
    pub(in crate::runner) fn ruling_posted(&mut self, sent: Sent) -> Option<String> {
        let Sent {
            id,
            thread,
            mut failed,
            comment,
            viewer,
            placed,
            unasked,
        } = sent;
        if let Some(login) = viewer {
            let _ = self.gh_login.set(login);
        }
        if let Some((number, comment)) = comment {
            self.keep_comment_id(number, comment);
        }
        if let Some(placed) = placed {
            let mut next = self.state.clone();
            placed.keep(&mut next, id);
            if let Err(e) = self.save(next) {
                self.notes.push(format!(
                    "cannot save where ruling {id} is answered in its thread: {e}"
                ));
            }
        }
        if let (Some(why), Some(thread)) = (unasked, thread) {
            failed = Some(self.tell_unmentioned(id, thread, &why));
        }
        failed
    }
}
