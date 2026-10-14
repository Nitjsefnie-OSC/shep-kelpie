//! What a test steps the runner with

use std::sync::{Mutex, mpsc};
use std::time::Duration;

use super::{Pass, Runner, StateError, StepReport, advance, brief, hear, lock};

/// Runs one pass, and waits for any call it starts to end and be recorded
///
/// What a test steps with: a pass that starts a call returns what its end
/// reports, as the loop would on the wake that brings it.
///
/// # Errors
///
/// [`StateError`] as [`advance`].
pub fn step(runner: &Mutex<Runner>) -> Result<Option<StepReport>, StateError> {
    let stepped = step_once(runner);
    brief(runner);
    // A notice hook runs off the runner's lock, so a test waits for what it told.
    lock(runner).settle_notices();
    stepped
}

fn step_once(runner: &Mutex<Runner>) -> Result<Option<StepReport>, StateError> {
    // A step waits on each call it starts in turn, so it keeps the loosest
    // ceiling the tests had before they shared `CALL_PATIENCE`.
    const PATIENCE: Duration = Duration::from_secs(120);
    let (wake, woken) = mpsc::channel();
    lock(runner).wake_with(wake);
    match advance(runner)? {
        Pass::Idle => return Ok(None),
        Pass::Report(report) => return Ok(Some(report)),
        Pass::Started => {}
    }
    loop {
        let news = lock(runner).next_news();
        match news {
            Some(news) => {
                if let Some(report) = hear(runner, news)? {
                    lock(runner).journal_report(&report);
                    return Ok(Some(report));
                }
            }
            None if lock(runner).flights.idle() => return Ok(None),
            None => woken
                .recv_timeout(PATIENCE)
                .expect("a call in flight never ended"),
        }
    }
}
