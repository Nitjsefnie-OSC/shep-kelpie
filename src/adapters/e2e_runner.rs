//! The project's e2e tests, run by asking the dog at its door
//!
//! The same ask `lease e2e` makes for a worker: the dog queues for the
//! project's host, runs the command there and streams its output back. The
//! dog ends a run at its own ceiling, so this waits as long as the dog does.

use std::io::{self, Write};

use crate::lease::cli::counted_run::NO_LEASE;
use crate::lease::cli::e2e::{NOT_CONFIGURED, run_printing};
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
        let code = runtime.block_on(run_printing(&run.door, run.worktree.clone(), &mut written));
        match code {
            0 => E2eOutcome::Passed,
            NOT_CONFIGURED => E2eOutcome::NoHost,
            NO_LEASE => E2eOutcome::NotRun {
                reason: "the dog was not reached, or went away before the run ended".to_owned(),
            },
            _ => E2eOutcome::Failed {
                output: tail(&written.0),
            },
        }
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
    fn only_the_end_of_a_long_output_is_kept() {
        let mut written = Tail::default();
        for n in 0..3000 {
            writeln!(written, "line {n}").unwrap();
        }

        assert!(written.0.len() <= OUTPUT_KEPT * 2);
        assert!(tail(&written.0).ends_with("line 2999\n"));
    }
}
