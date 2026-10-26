//! Draining a runner before its restart or its stop, so neither cuts a call
//! short
//!
//! The runner is sent `drain`, after which it starts no new call, and asked
//! again until it shows none running. `upgrade` drains a runner before its
//! restart, and `shep kelpie pause` before its stop ([`Purpose`]). The wait
//! is bounded by the longest a call of the runner's may run, which its
//! answer says, plus [`Patience::margin`], counted from when the wait began.
//! A turn's ceiling counts from when it got the lease it waited for, and a
//! reviewer's call has none, so either can outrun the bound. Past it the runner is named and
//! left running.
//!
//! A pause sends `drain pause`, which also ends each worker turn at its next
//! tool call, on a harness that can. Those turns are waited on for
//! [`Patience::tool`] only: a tool call can run long, and past the limit the
//! pause goes on to the stop, which cuts the call short as `shep stop` does.
//! A runner from before `drain pause` is sent a plain `drain` and waited on
//! as an upgrade waits. A runner on a build from before `drain` answers it, twice,
//! as an action nobody took while it answers `status`: it is restarted or
//! stopped as before, which cuts its calls short.

use std::time::Duration;

use serde_json::Value;
use tokio::time::Instant;

use super::restart::Patience;
use crate::flock::control::{Answered, refusal, trigger};
use crate::shepherd::Connection;
use crate::ticket::TicketId;

/// What a runner is drained for, which the lines about it name
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    /// `shep kelpie upgrade`, which restarts it on the new build
    Upgrade,
    /// `shep kelpie pause`, which stops it
    Pause,
}

impl Purpose {
    // The command, as "if this ... stops first" names it.
    fn command(self) -> &'static str {
        match self {
            Self::Upgrade => "upgrade",
            Self::Pause => "pause",
        }
    }

    // What the runner waits for once drained.
    fn until(self) -> &'static str {
        match self {
            Self::Upgrade => "until its restart",
            Self::Pause => "until it stops",
        }
    }

    /// What was not done to a runner the wait ran out on
    pub(crate) fn not_done(self) -> &'static str {
        match self {
            Self::Upgrade => "nothing was restarted for it",
            Self::Pause => "it was not stopped",
        }
    }

    // The params its `drain` takes: a pause also ends worker turns at a tool call.
    fn params(self) -> Option<&'static str> {
        match self {
            Self::Upgrade => None,
            Self::Pause => Some("pause"),
        }
    }

    // The params its `undrain` takes: a pause that gives up leaves the turns it ended stopped.
    fn undrain_params(self) -> Option<&'static str> {
        match self {
            Self::Upgrade => None,
            Self::Pause => Some("keep"),
        }
    }

    // What ends a runner's calls without a drain.
    fn cut(self) -> &'static str {
        match self {
            Self::Upgrade => "its restart",
            Self::Pause => "its stop",
        }
    }
}

/// What a draining runner's answer says
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drained {
    /// Each call still running, as the upgrade names it, but those a pause
    /// ends at a tool call
    pub calls: Vec<String>,
    /// The longest a call of the runner's may run, in seconds
    pub ceiling: u64,
    /// Each worker turn still running that ends at its next tool call
    pub at_tool: Vec<String>,
}

/// Reads a runner's answer to `drain`: `None` when it is not draining, as a
/// runner from before `drain` is not
pub fn drained(body: &str) -> Option<Drained> {
    let answer = serde_json::from_str::<Value>(body).ok()?;
    let draining = answer.get("draining")?;
    let calls = draining.get("calls")?.as_array()?;
    let at_tool: Vec<TicketId> = draining
        .get("at_tool")
        .and_then(Value::as_array)
        .map(|tickets| tickets.iter().filter_map(TicketId::from_json).collect())
        .unwrap_or_default();
    let ends_at_tool = |call: &&Value| {
        call["role"] == "worker"
            && TicketId::from_json(&call["issue"]).is_some_and(|issue| at_tool.contains(&issue))
    };
    let (ending, others): (Vec<_>, Vec<_>) = calls.iter().partition(ends_at_tool);
    Some(Drained {
        calls: others.into_iter().map(call).collect(),
        ceiling: draining.get("ceiling")?.as_u64()?,
        at_tool: (ending.into_iter())
            .map(|ending| format!("{}, which ends at its next tool call", call(ending)))
            .collect(),
    })
}

// How a call in a draining runner's answer reads. A runner from before ticket
// ids names a call by its issue number, which reads as GitHub's.
fn call(call: &Value) -> String {
    let ticket = TicketId::from_json(&call["issue"]);
    match (call["role"].as_str(), ticket) {
        (Some("pm"), _) => "the project manager's wake".to_owned(),
        (Some("panel"), _) => "the panel's votes on a ruling".to_owned(),
        (Some("worker"), Some(ticket)) => format!("{}'s worker turn", ticket.shown()),
        (Some("pi_session"), Some(ticket)) => format!("{}'s attached pi session", ticket.shown()),
        (Some("goose_session"), Some(ticket)) => {
            format!("{}'s attached goose session", ticket.shown())
        }
        (Some("codex_session"), Some(ticket)) => {
            format!("{}'s attached codex session", ticket.shown())
        }
        (Some("reviewer"), Some(ticket)) => format!("{}'s review", ticket.shown()),
        (_, Some(ticket)) => format!("a call for {}", ticket.shown()),
        (_, None) => "a call".to_owned(),
    }
}

/// Sends `name` `action`, and `None` when it is a build from before it
///
/// An action nobody took is a runner still opening, or one from before the
/// action. One that answers `status` and then the same again is old.
pub(super) async fn asked(
    client: &dyn Connection,
    name: &str,
    action: &str,
    params: Option<&str>,
) -> Result<Option<Answered>, String> {
    let mut answer = trigger(client, name, action, params).await?;
    if matches!(answer, Answered::Starting)
        && matches!(
            trigger(client, name, "status", None).await?,
            Answered::Runner(_)
        )
    {
        answer = trigger(client, name, action, params).await?;
        if matches!(answer, Answered::Starting) {
            return Ok(None);
        }
    }
    Ok(Some(answer))
}

/// Drains `name` for `purpose` and returns once it has no call running,
/// saying what it waits on through `say`
///
/// A runner that is not running, or runs a build from before `drain`, is
/// not waited on.
///
/// # Errors
///
/// A message naming the runner and what it still runs when the wait runs
/// out, or one from the shepherd. A runner that never says it drains is
/// waited on for `patience.merge`, as one that never answers `status` is.
/// The runner is left draining: the caller sends it [`undrain`].
pub async fn drain(
    client: &dyn Connection,
    name: &str,
    patience: Patience,
    purpose: Purpose,
    say: &mut dyn FnMut(String),
) -> Result<(), String> {
    let waited = Instant::now();
    let mut limit = patience.merge;
    let mut told = Vec::new();
    let mut began = false;
    let mut params = purpose.params();
    let mut cut = false;
    loop {
        // Set while only turns that end at a tool call are waited on, which `patience.tool` bounds.
        let mut tool_only = false;
        let Some(answer) = asked(client, name, "drain", params).await? else {
            return predates(name, purpose, say);
        };
        let busy = match answer {
            Answered::Runner(body) => match drained(&body) {
                // A runner from before `drain pause` refuses the params, and
                // takes a plain `drain`.
                None if params.is_some() && refusal(&body).is_some() => {
                    params = None;
                    continue;
                }
                Some(drained) => {
                    if !began {
                        began = true;
                        say(format!(
                            "draining `{name}`, which starts no new call {}: if this {} stops \
                             first, `shep kelpie undrain -p {name}` lets it start them again",
                            purpose.until(),
                            purpose.command()
                        ));
                    }
                    limit = Duration::from_secs(drained.ceiling).saturating_add(patience.margin);
                    let running = |call| format!("`{name}` is running {call}");
                    // A tool call is waited on for `patience.tool` only. Past
                    // it the stop cuts the call short, as `shep stop` does.
                    let past = waited.elapsed() >= patience.tool;
                    tool_only = drained.calls.is_empty();
                    let mut busy: Vec<String> = drained.calls.into_iter().map(running).collect();
                    if past && !drained.at_tool.is_empty() && !cut {
                        cut = true;
                        say(format!(
                            "`{name}` is still in a tool call of {} after {}s, so the stop cuts \
                             {} short",
                            drained.at_tool.join(" and "),
                            patience.tool.as_secs(),
                            if drained.at_tool.len() == 1 {
                                "it"
                            } else {
                                "them"
                            }
                        ));
                    }
                    if !past {
                        busy.extend(drained.at_tool.into_iter().map(running));
                    }
                    busy
                }
                None => return predates(name, purpose, say),
            },
            Answered::Starting => vec![format!("`{name}` is starting")],
            Answered::TimedOut => vec![format!("`{name}` did not answer `drain`")],
            Answered::Down => return Ok(()),
        };
        if busy.is_empty() {
            return Ok(());
        }
        if !tool_only && waited.elapsed() >= limit {
            return Err(format!(
                "{name}'s runner was busy, not down, and answered slowly: {} after {}s, so {}",
                busy.join(" and "),
                waited.elapsed().as_secs(),
                purpose.not_done()
            ));
        }
        if told != busy {
            say(format!("waiting: {}", busy.join(" and ")));
            told = busy;
        }
        tokio::time::sleep(patience.poll).await;
    }
}

fn predates(name: &str, purpose: Purpose, say: &mut dyn FnMut(String)) -> Result<(), String> {
    say(format!(
        "`{name}` runs a kelpie from before `drain`, so {} cuts short any call it has running",
        purpose.cut()
    ));
    Ok(())
}

/// Sends `name` `undrain`, and says so for a message when it was answered
///
/// A pause sends `undrain keep`, so the turns it ended stay stopped for a
/// while. A runner from before `keep` refuses the param and is sent a plain
/// `undrain`. A failure to send it is let go: only a shepherd that cannot be
/// reached leaves the runner draining, and its restart ends that.
pub async fn undrain(client: &dyn Connection, name: &str, purpose: Purpose) -> String {
    let on = match purpose {
        Purpose::Upgrade => " on the old build",
        Purpose::Pause => "",
    };
    let mut answer = trigger(client, name, "undrain", purpose.undrain_params()).await;
    if purpose.undrain_params().is_some()
        && let Ok(Answered::Runner(body)) = &answer
        && refusal(body).is_some()
    {
        answer = trigger(client, name, "undrain", None).await;
    }
    match answer {
        Ok(Answered::Runner(_)) => {
            format!(", and it was sent `undrain`, so it starts calls again{on}")
        }
        _ => String::new(),
    }
}
