//! `lease e2e`: a project's end-to-end tests, run by the dog on the project's host
//!
//! The ask carries the folder the command runs from and nothing else. The
//! dog works out the project from that folder, and takes the host from the
//! project's own settings, so a worker cannot name one.

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
/// `worktree`, and returns the number to exit with
pub(crate) async fn run(socket: &Path, worktree: PathBuf) -> u8 {
    let ask = Ask {
        take: E2E.into(),
        pid: std::process::id(),
        what: "shep-kelpie lease e2e".into(),
        running: false,
        worktree: Some(worktree),
    };
    let mut visit = match Visit::knock(socket, &ask).await {
        Ok(visit) => visit,
        Err(why) => return failed(&unreached(&why), NO_LEASE),
    };
    match tokio::time::timeout(ANSWER_WITHIN, visit.answer()).await {
        Err(_) => failed("the dog took the ask and never answered", NO_LEASE),
        Ok(Err(why)) => failed(&unreached(&why), NO_LEASE),
        Ok(Ok(Some(Answer::Error(why)))) if why.starts_with(NO_E2E_HOST) => {
            failed(&why, NOT_CONFIGURED)
        }
        Ok(Ok(Some(Answer::Error(why)))) => failed(&format!("the dog refused: {why}"), 1),
        Ok(Ok(Some(other))) => failed(&format!("the dog answered {other:?}"), 1),
        Ok(Ok(None)) => failed("the dog closed the door without answering", NO_LEASE),
    }
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
    async fn lease_e2e_exits_1_when_the_dog_refuses_for_any_other_reason() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("lease.sock");
        let _dog = dog_answering(
            &socket,
            r#"{"error":"this dog cannot run e2e yet: update kelpie and restart the dog"}"#,
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
