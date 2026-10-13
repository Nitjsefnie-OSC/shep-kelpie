//! Handing a runner over to the installed build instead of restarting it
//!
//! The runner is sent `handover` and replaces its own image at its quiet
//! point, keeping its pid, its channel and the calls in flight. The upgrade
//! then asks `status` until the runner shows the installed file's stamp
//! ([`super::seen`]). A runner that refuses, predates the trigger, or does not
//! show the new build in time is left for the caller to drain and restart.

use serde_json::Value;
use tokio::time::Instant;

use super::drain::asked;
use super::restart::{Patience, running};
use crate::flock::control::{Answered, trigger};
use crate::flock::flock;
use crate::shepherd::Connection;

/// How a runner took its handover
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handed {
    /// It runs the installed build now, or was stopped and stays so
    Over,
    /// It stays on the old build, for the reason the line says: drain it
    Stays(String),
}

/// Hands `name` over to the build whose file stamps as `build`, saying what
/// it waits on through `say`
///
/// A runner still opening its channel is asked again until `patience.start`
/// runs out, then left for the caller to drain like one that refuses.
///
/// # Errors
///
/// A message from the shepherd.
pub async fn hand_over(
    client: &dyn Connection,
    name: &str,
    build: &str,
    patience: Patience,
    say: &mut dyn FnMut(String),
) -> Result<Handed, String> {
    let stays = |why: String| {
        Ok(Handed::Stays(format!(
            "{why}, so it is drained and restarted as before"
        )))
    };
    let waited = Instant::now();
    loop {
        match asked(client, name, "handover").await? {
            None => return stays(format!("`{name}` runs a kelpie from before `handover`")),
            Some(Answered::Down) => return stopped(name, say),
            Some(Answered::Runner(body)) => {
                if let Some(why) = refusal(&body) {
                    return stays(format!("`{name}` refused to hand over: {why}"));
                }
                break;
            }
            // The old image replaces itself as it takes the trigger, so no
            // answer is the usual one.
            Some(Answered::TimedOut) => break,
            // Not taken, and `status` did not answer either: still opening.
            Some(Answered::Starting) => {
                if waited.elapsed() >= patience.start {
                    return stays(format!(
                        "`{name}` did not take the handover within {:?}",
                        patience.start
                    ));
                }
                tokio::time::sleep(patience.poll).await;
            }
        }
    }
    let waited = Instant::now();
    let mut told = false;
    // What the last `status` showed of the build, for the failure to say.
    let mut shown = None;
    loop {
        match trigger(client, name, "status", None).await? {
            Answered::Runner(body) => {
                let status = serde_json::from_str::<Value>(&body).unwrap_or_default();
                if status["build"].as_str() == Some(build) {
                    say(format!("`{name}` handed over to the new build"));
                    return Ok(Handed::Over);
                }
                shown = Some(status["build"].as_str().is_some());
            }
            Answered::Down if !runs(client, name).await? => return stopped(name, say),
            _ => {}
        }
        if waited.elapsed() >= patience.start {
            let seen = match shown {
                None => "answered no `status`",
                Some(true) => "still showed the old build",
                Some(false) => "showed no build in its `status`",
            };
            return stays(format!(
                "`{name}` {seen} within {:?} of its handover",
                patience.start
            ));
        }
        if !told {
            say(format!("waiting: `{name}` is handing over"));
            told = true;
        }
        tokio::time::sleep(patience.poll).await;
    }
}

// A runner that is not running stays stopped: it starts on the new build.
fn stopped(name: &str, say: &mut dyn FnMut(String)) -> Result<Handed, String> {
    say(format!(
        "`{name}` was stopped meanwhile, so it stays stopped"
    ));
    Ok(Handed::Over)
}

// Whether the shepherd has `name` running.
async fn runs(client: &dyn Connection, name: &str) -> Result<bool, String> {
    Ok(flock(client)
        .await?
        .into_iter()
        .find(|row| row.name == name)
        .is_some_and(|row| running(row.status)))
}

// Why a runner refused: the message its `error` carries, without the
// "handover refused: " it opens with.
fn refusal(body: &str) -> Option<String> {
    let answer = serde_json::from_str::<Value>(body).ok()?;
    let error = answer.get("error")?.as_str()?;
    Some(
        error
            .strip_prefix("handover refused: ")
            .unwrap_or(error)
            .to_owned(),
    )
}
