//! The project's e2e tests, run by asking the dog at its door
//!
//! The same ask `lease e2e` makes for a worker: the dog queues for the
//! project's host, runs the command there and streams its output back. The
//! dog ends a run at its own ceiling, so this waits as long as the dog does.

use std::io::{self, Write};

use crate::lease::cli::e2e::{Ended, run_printing};
use crate::ports::{E2eOutcome, E2eRun, E2eRunner, OUTPUT_KEPT, tail};

/// Runs the e2e tests through the dog
#[derive(Debug, Clone, Copy, Default)]
pub struct DogE2e;

impl E2eRunner for DogE2e {
    fn run(&self, run: &E2eRun) -> E2eOutcome {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(e) => {
                return E2eOutcome::NotRun {
                    reason: format!("cannot start the async runtime: {e}"),
                };
            }
        };
        let mut written = Tail::default();
        let ended = runtime.block_on(run_printing(&run.door, run.worktree.clone(), &mut written));
        outcome_of(ended, &written.0)
    }
}

// What the run came to: only a command that ran and exited non-zero failed.
fn outcome_of(ended: Ended, written: &[u8]) -> E2eOutcome {
    match ended {
        Ended::Exited(0) => E2eOutcome::Passed,
        Ended::Exited(_) => E2eOutcome::Failed {
            output: tail(written),
        },
        Ended::NoHost(_) => E2eOutcome::NoHost,
        Ended::Unreached(_) => E2eOutcome::NotRun {
            reason: "the dog was not reached, or went away before the run ended".to_owned(),
        },
        Ended::Refused(why) => E2eOutcome::NotRun { reason: why },
    }
}

// The end of what a run wrote: a run of half an hour is not held whole.
#[derive(Debug, Default)]
struct Tail(Vec<u8>);

impl Write for Tail {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.extend_from_slice(bytes);
        let over = self.0.len().saturating_sub(OUTPUT_KEPT * 2);
        self.0.drain(..over);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_run_the_dog_cannot_be_asked_for_is_not_run() {
        let dir = tempfile::tempdir().unwrap();
        let run = E2eRun {
            worktree: dir.path().to_owned(),
            door: dir.path().join("lease.sock"),
        };

        let outcome = DogE2e.run(&run);

        assert!(matches!(outcome, E2eOutcome::NotRun { .. }), "{outcome:?}");
    }

    #[test]
    fn only_a_command_that_ran_and_failed_is_a_failed_run() {
        let failed = |code| outcome_of(Ended::Exited(code), b"not ok\n");
        // 75 and 78 are the dog's codes, but here they are the command's own.
        for code in [1, 75, 78] {
            assert_eq!(
                failed(code),
                E2eOutcome::Failed {
                    output: "not ok\n".to_owned()
                }
            );
        }
        let refused = outcome_of(Ended::Refused("the dog refused".into()), b"");
        assert!(matches!(refused, E2eOutcome::NotRun { .. }), "{refused:?}");
        let gone = outcome_of(Ended::Unreached("gone".into()), b"");
        assert!(matches!(gone, E2eOutcome::NotRun { .. }), "{gone:?}");
        assert_eq!(
            outcome_of(Ended::NoHost("none".into()), b""),
            E2eOutcome::NoHost
        );
        assert_eq!(outcome_of(Ended::Exited(0), b""), E2eOutcome::Passed);
    }

    #[test]
    fn only_the_end_of_a_long_output_is_kept() {
        let mut written = Tail::default();
        for n in 0..3000 {
            writeln!(written, "line {n}").unwrap();
        }

        assert!(written.0.len() <= OUTPUT_KEPT * 2);
        assert!(tail(&written.0).ends_with("line 2999\n"));
    }
}
