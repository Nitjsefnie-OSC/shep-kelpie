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
use super::restart::Patience;
use crate::flock::control::{Answered, trigger};
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
    match asked(client, name, "handover").await? {
        None => return stays(format!("`{name}` runs a kelpie from before `handover`")),
        Some(Answered::Down) => {
            say(format!(
                "`{name}` was stopped meanwhile, so it stays stopped"
            ));
            return Ok(Handed::Over);
        }
        Some(Answered::Runner(body)) => {
            if let Some(why) = refusal(&body) {
                return stays(format!("`{name}` refused to hand over: {why}"));
            }
        }
        // The old image replaces itself as it takes the trigger, so no answer
        // is the usual one; a runner still opening gave none either.
        Some(Answered::TimedOut | Answered::Starting) => {}
    }
    let waited = Instant::now();
    let mut told = false;
    loop {
        if let Answered::Runner(body) = trigger(client, name, "status", None).await?
            && shows(&body, build)
        {
            say(format!("`{name}` handed over to the new build"));
            return Ok(Handed::Over);
        }
        if waited.elapsed() >= patience.start {
            return stays(format!(
                "`{name}` did not show the new build within {}s of its handover",
                patience.start.as_secs()
            ));
        }
        if !told {
            say(format!("waiting: `{name}` is handing over"));
            told = true;
        }
        tokio::time::sleep(patience.poll).await;
    }
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

// Whether a runner's `status` shows the build with this stamp.
fn shows(status: &str, build: &str) -> bool {
    serde_json::from_str::<Value>(status)
        .ok()
        .is_some_and(|status| status["build"].as_str() == Some(build))
}
