//! `lease e2e`: a project's end-to-end tests, run by the dog on the project's host
//!
//! The ask carries the folder the command runs from and nothing else. The
//! dog works out the project from that folder, and takes the host from the
//! project's own settings, so a worker cannot name one.
//! The dog streams the command's output back, a line at a time, and ends
//! with its exit code.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::counted_run::NO_LEASE;
use crate::lease::door::{Answer, Ask, E2E, NO_E2E_HOST, Visit};

/// What the command exits with when the dog says the project has no e2e
/// host, as `EX_CONFIG`
pub(crate) const NOT_CONFIGURED: u8 = 78;

/// How long a dog has to answer; a live one answers at once
const ANSWER_WITHIN: Duration = Duration::from_secs(5);

/// Asks the dog at `socket` to run the e2e tests for the project holding
/// `worktree`, prints the command's output as it arrives, and returns the
/// number to exit with: the command's own, or [`NO_LEASE`] when the dog is
/// not reached or goes away
pub(crate) async fn run(socket: &Path, worktree: PathBuf) -> u8 {
    run_printing(socket, worktree, &mut std::io::stdout()).await
}

// `run`, printing the command's lines to `out`.
async fn run_printing(socket: &Path, worktree: PathBuf, out: &mut impl Write) -> u8 {
    let ask = Ask {
        take: E2E.into(),
        pid: std::process::id(),
        what: "shep-kelpie lease e2e".into(),
        running: false,
        worktree: Some(worktree),
        forge: None,
    };
    let visit = match Visit::knock(socket, &ask).await {
        Ok(visit) => visit,
        Err(why) => return failed(&unreached(&why), NO_LEASE),
    };
    follow(visit, out).await
}

// The dog's answers to an ask, printed and turned into a code.
async fn follow(mut visit: Visit, out: &mut impl Write) -> u8 {
    let first = match tokio::time::timeout(ANSWER_WITHIN, visit.answer()).await {
        Ok(answer) => answer,
        Err(_) => return failed("the dog took the ask and never answered", NO_LEASE),
    };
    let mut granted = false;
    let mut next = first;
    loop {
        match next {
            Err(why) if granted => return failed(&gone(&why), NO_LEASE),
            Err(why) => return failed(&unreached(&why), NO_LEASE),
            Ok(None) if granted => return failed(&gone("the dog closed the door"), NO_LEASE),
            Ok(None) => {
                return failed("the dog closed the door without answering", NO_LEASE);
            }
            Ok(Some(Answer::Queued(ahead))) => {
                eprintln!("shep kelpie lease: waiting for the e2e host, {ahead} ahead");
            }
            Ok(Some(Answer::Granted)) => granted = true,
            Ok(Some(Answer::Output(line))) => {
                // A reader that went away leaves the run going; its code still counts.
                let _ = writeln!(out, "{line}").and_then(|()| out.flush());
            }
            Ok(Some(Answer::Done(code))) => return u8::try_from(code).unwrap_or(1),
            Ok(Some(Answer::Error(why))) if why.starts_with(NO_E2E_HOST) => {
                return failed(&why, NOT_CONFIGURED);
            }
            Ok(Some(Answer::Error(why))) => {
                return failed(&format!("the dog refused: {why}"), 1);
            }
            Ok(Some(other @ (Answer::Go | Answer::Wait(_) | Answer::Held(_)))) => {
                return failed(&format!("the dog answered {other:?} to the e2e ask"), 1);
            }
        }
        next = visit.answer().await;
    }
}

// What a run says when the dog goes away after granting the host: the
// command's end is then unknown, and the dog ended the run with its worker.
fn gone(why: &str) -> String {
    format!(
        "{why} before the e2e run ended: the e2e tests did not finish, so say so in your final message"
    )
}

// A worker cannot start the dog, so it is told to go on without the run.
fn unreached(why: &str) -> String {
    format!("{why}: the e2e tests did not run, so say so in your final message")
}

fn failed(message: &str, code: u8) -> u8 {
    eprintln!("shep kelpie lease: {message}");
    code
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixListener;

    use super::*;

    // A door that reads one ask and answers it with `answer`, handing back
    // the ask it read.
    fn dog_answering(socket: &Path, answer: &'static str) -> tokio::task::JoinHandle<Option<Ask>> {
        let listener = UnixListener::bind(socket).unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.ok()?;
            let (read, mut write) = stream.into_split();
            let line = BufReader::new(read).lines().next_line().await.ok()??;
            write
                .write_all(format!("{answer}\n").as_bytes())
                .await
                .ok()?;
            serde_json::from_str(&line).ok()
        })
    }

    #[tokio::test]
    async fn lease_e2e_exits_78_when_the_dog_says_it_is_not_configured() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("lease.sock");
        let dog = dog_answering(
            &socket,
            r#"{"error":"this project has no e2e host: [app.dogs.kelpie.e2e]"}"#,
        );

        let code = run(&socket, dir.path().join("worktree")).await;

        assert_eq!(code, 78);
        let heard = dog.await.unwrap().expect("the dog read an ask");
        assert_eq!(heard.take, "e2e");
        assert_eq!(heard.worktree, Some(dir.path().join("worktree")));
    }

    #[tokio::test]
    async fn lease_e2e_prints_the_output_and_exits_with_the_runs_code() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("lease.sock");
        let _dog = dog_answering(
            &socket,
            "{\"queued\":1}\n\"granted\"\n{\"output\":\"ok 1\"}\n{\"output\":\"not ok 2\"}\n{\"done\":3}",
        );
        let mut printed = Vec::new();

        let code = run_printing(&socket, dir.path().to_owned(), &mut printed).await;

        assert_eq!(code, 3);
        assert_eq!(String::from_utf8(printed).unwrap(), "ok 1\nnot ok 2\n");
    }

    #[tokio::test]
    async fn lease_e2e_exits_75_when_the_dog_goes_away_mid_run() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("lease.sock");
        let _dog = dog_answering(&socket, "\"granted\"\n{\"output\":\"ok 1\"}");
        let mut printed = Vec::new();

        let code = run_printing(&socket, dir.path().to_owned(), &mut printed).await;

        assert_eq!(code, 75);
        assert_eq!(String::from_utf8(printed).unwrap(), "ok 1\n");
    }

    #[tokio::test]
    async fn lease_e2e_exits_1_when_the_dog_refuses_for_any_other_reason() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("lease.sock");
        let _dog = dog_answering(
            &socket,
            r#"{"error":"a results folder in the worktree is a link"}"#,
        );

        assert_eq!(run(&socket, dir.path().to_owned()).await, 1);
    }

    #[tokio::test]
    async fn lease_e2e_exits_75_when_the_dog_is_not_reached() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("lease.sock");

        assert_eq!(run(&socket, dir.path().to_owned()).await, 75);
    }
}
