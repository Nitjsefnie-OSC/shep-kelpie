//! `shep kelpie lease`: the maintainer's lease commands
//!
//! `run` waits for a lease, runs a command and returns the lease when the
//! command exits, however it exits. `take` and `return` hold one for
//! longer. `cargo-test` is held for one `run` at the dog's door. Every
//! other kind is the dog's, reached through shep. The GPU has no lease
//! here: `gpu` is refused, and paddock holds a model.

use std::process::{ExitCode, ExitStatus};
use std::time::Duration;

use serde_json::Value;
use shep_client::shep_core::protocol::Request;
use shep_client::shep_core::protocol::request::{ActionOutcome, Response, SelectorSpec};

use super::counted::CARGO_TEST;
use super::door;
use super::{GPU, GPU_GONE, LeaseKind};
use crate::interrupt::{Caught, Interrupts};
use crate::tree::Tree;
use crate::{home, shepherd, status};

pub mod counted_run;
pub mod e2e;

/// The usage lines for `shep-kelpie lease`
pub const USAGE: &str = "\
       shep-kelpie lease run <kind> -- <command> [args...]
       shep-kelpie lease take <kind>
       shep-kelpie lease return <kind>
       shep-kelpie lease status
       shep-kelpie lease e2e
       <kind> is cargo-test, which a few commands hold at once and only for a run, or
       a lease the dog holds. The GPU is no lease: `shep paddock run --model <model>
       -- <command>` holds a model
       e2e runs the project's end-to-end tests on its host, from a worktree, and
       prints their output and exits with their code";

/// How often a waiting maintainer asks the dog again; a local round trip
/// through shep took about 30 ms in the transport series
const DOG_POLL: Duration = Duration::from_secs(1);

/// Runs `shep kelpie lease <args>`
pub fn main(args: &[String]) -> ExitCode {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build();
    let outcome = match runtime {
        Ok(runtime) => runtime.block_on(dispatch(args)),
        Err(e) => Err(format!("cannot start the async runtime: {e}")),
    };
    match outcome {
        Ok(code) => code,
        Err(message) => {
            eprintln!("shep kelpie lease: {message}");
            ExitCode::FAILURE
        }
    }
}

async fn dispatch(args: &[String]) -> Result<ExitCode, String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["run" | "take" | "return" | "hold", GPU, ..] => Err(GPU_GONE.into()),
        ["run", CARGO_TEST, "--", command @ ..] if !command.is_empty() => {
            let code = match door::socket() {
                Ok(socket) => counted_run::run(&socket, command, counted_run::RUN_LIMIT).await,
                Err(e) => {
                    eprintln!("shep kelpie lease: {e}");
                    counted_run::NO_LEASE
                }
            };
            Ok(ExitCode::from(code))
        }
        ["run", kind, "--", command @ ..] if !command.is_empty() => {
            run_book(&kind_of(kind)?, command).await
        }
        ["take", kind] => take_book(&kind_of(kind)?).await,
        ["return", kind] => return_book(&kind_of(kind)?).await,
        ["status"] => status().await,
        ["e2e"] => run_e2e().await,
        _ => {
            eprintln!("usage: {}", USAGE.trim_start());
            Ok(ExitCode::from(2))
        }
    }
}

// Runs the project's e2e tests from the folder it is in, which the dog
// places by the worktree that holds it.
async fn run_e2e() -> Result<ExitCode, String> {
    let worktree = std::env::current_dir()
        .map_err(|e| format!("cannot tell which folder this runs from: {e}"))?;
    let socket = door::socket()?;
    Ok(ExitCode::from(e2e::run(&socket, worktree).await))
}

fn kind_of(kind: &str) -> Result<LeaseKind, String> {
    LeaseKind::try_from(kind).map_err(|e| e.to_string())
}

async fn run_book(kind: &LeaseKind, command: &[&str]) -> Result<ExitCode, String> {
    let mut signals = listen()?;
    if let Some(caught) = wait_for_grant(kind, &mut signals).await? {
        return Ok(exit_code_of(caught));
    }
    let ran = run_command(command, &mut signals).await;
    let returned = ask_dog("return", kind.as_str())
        .await
        .map_err(|e| format!("{e}: run `shep kelpie lease return {kind}`"));
    both(ran, returned)
}

// The command's outcome and its lease's return, with both errors if both failed.
fn both<R, T>(ran: Result<R, String>, back: Result<T, String>) -> Result<R, String> {
    match (ran, back) {
        (Ok(outcome), Ok(_)) => Ok(outcome),
        (Err(e), Ok(_)) | (Ok(_), Err(e)) => Err(e),
        (Err(ran), Err(back)) => Err(format!("{ran}, and {back}")),
    }
}

async fn take_book(kind: &LeaseKind) -> Result<ExitCode, String> {
    let mut signals = listen()?;
    if let Some(caught) = wait_for_grant(kind, &mut signals).await? {
        return Ok(exit_code_of(caught));
    }
    println!("shep kelpie lease: {kind} is yours until `shep kelpie lease return {kind}`");
    Ok(ExitCode::SUCCESS)
}

async fn return_book(kind: &LeaseKind) -> Result<ExitCode, String> {
    let reply = ask_dog("return", kind.as_str()).await?;
    if reply["returned"] == true {
        println!("shep kelpie lease: returned {kind}");
    } else {
        println!("shep kelpie lease: you did not hold {kind}, and any wait for it is withdrawn");
    }
    Ok(ExitCode::SUCCESS)
}

// Asks the dog until it grants, or withdraws the ask on a signal and
// returns it.
async fn wait_for_grant(
    kind: &LeaseKind,
    signals: &mut Interrupts,
) -> Result<Option<Caught>, String> {
    let mut last_ahead = None;
    loop {
        // A signal cuts the ask short too, not only the nap between asks.
        let reply = tokio::select! {
            reply = ask_dog("take", kind.as_str()) => reply?,
            caught = signals.recv() => return Ok(Some(withdraw(kind, caught).await)),
        };
        if reply["granted"] == true {
            return Ok(None);
        }
        let ahead = reply["queued"].as_u64();
        let ahead = ahead.ok_or_else(|| format!("the dog answered {reply}"))?;
        if last_ahead != Some(ahead) {
            eprintln!("shep kelpie lease: waiting for {kind}, {ahead} ahead");
            last_ahead = Some(ahead);
        }
        tokio::select! {
            () = tokio::time::sleep(DOG_POLL) => {}
            caught = signals.recv() => return Ok(Some(withdraw(kind, caught).await)),
        }
    }
}

// Withdraws the ask, or says how to, and hands the signal back.
async fn withdraw(kind: &LeaseKind, caught: Caught) -> Caught {
    if let Err(e) = ask_dog("return", kind.as_str()).await {
        eprintln!("shep kelpie lease: {e}: run `shep kelpie lease return {kind}`");
    }
    caught
}

async fn status() -> Result<ExitCode, String> {
    let status = ask_dog("status", "").await?;
    println!("{status}");
    Ok(ExitCode::SUCCESS)
}

// Sends a trigger to the dog and returns its JSON reply, or its error.
async fn ask_dog(action: &str, params: &str) -> Result<Value, String> {
    let shep_home = home::shep_home()?;
    let client = shepherd::connect_any(&shepherd::Socket, &shep_home)
        .await
        .map_err(|e| e.describe(&shep_home))?;
    let asked = client.request(Request::Trigger {
        selector: SelectorSpec::Name(shepherd::DOG.into()),
        action: action.into(),
        params: Some(params.to_owned()).filter(|p| !p.is_empty()),
    });
    let rows = match asked.await {
        Ok(Response::Triggered(rows)) => rows,
        Err(e) if shepherd::names_no_sheep(&e) => Vec::new(),
        Ok(other) => return Err(format!("the shepherd answered {other:?}")),
        Err(e) => return Err(format!("cannot ask the dog: {e}")),
    };
    let body = match rows.into_iter().next().map(|row| row.outcome) {
        Some(ActionOutcome::Replied { body }) => body,
        Some(ActionOutcome::DogNoChannel) => return Err(shepherd::no_channel()),
        Some(other) => return Err(format!("the dog did not answer: {other:?}")),
        None => {
            return Err(format!(
                "kelpie's dog is not enabled: `shep enable {}` runs it",
                shepherd::DOG
            ));
        }
    };
    let value: Value =
        serde_json::from_str(&body).map_err(|e| format!("the dog answered {body:?} ({e})"))?;
    match value.get("error") {
        None | Some(Value::Null) => Ok(value),
        Some(Value::String(error)) => Err(format!("the dog refused: {error}")),
        Some(error) => Err(format!("the dog refused: {error}")),
    }
}

// Runs `command` to its end. A hangup or terminate is passed on to it; an
// interrupt from the terminal has already reached it. A signal that came
// before the command started reached no one else, so it ends the run
// without starting it.
async fn run_command(command: &[&str], signals: &mut Interrupts) -> Result<ExitCode, String> {
    let (program, args) = command.split_first().ok_or("no command to run")?;
    if let Some(caught) = signals.already().await {
        return Ok(exit_code_of(caught));
    }
    let mut child = tokio::process::Command::from(crate::spawn::command(program))
        .args(args)
        .spawn()
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    // The command shares this process's group, so a signal reaches it alone.
    let tree = Tree::of_tokio(&child).ok().map(Tree::alone);
    loop {
        tokio::select! {
            status = child.wait() => return status.map(exit_code).map_err(|e| e.to_string()),
            caught = signals.recv() => {
                if let (Some(tree), Some(signal)) = (&tree, caught.forward()) {
                    tree.send(signal);
                }
            }
        }
    }
}

fn exit_code(status: ExitStatus) -> ExitCode {
    ExitCode::from(exit_number(status))
}

// The shell's number for how a command ended: its code, or 128 plus its signal.
fn exit_number(status: ExitStatus) -> u8 {
    u8::try_from(status::shell_number(status).unwrap_or(1)).unwrap_or(1)
}

fn exit_code_of(caught: Caught) -> ExitCode {
    ExitCode::from(caught.number())
}

// Listening replaces each signal's default of ending kelpie at once, so
// a lease is returned however the command ends.
fn listen() -> Result<Interrupts, String> {
    Interrupts::listen(&[Caught::Interrupt, Caught::Terminate, Caught::Hangup])
        .map_err(|e| format!("cannot listen for signals: {e}"))
}

#[cfg(test)]
mod tests {
    use shep_kelpie_test_support::exited;

    use super::*;

    // A caught signal reaches every run alive when it lands, so the tests
    // that listen for signals take turns.
    #[cfg(unix)]
    pub(super) static LISTENING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn words(args: &[&str]) -> Vec<String> {
        args.iter().map(|&a| a.to_owned()).collect()
    }

    #[test]
    fn both_keeps_every_error() {
        let ok = || Ok::<_, String>(exited(0));
        assert_eq!(both(ok(), Ok::<(), String>(())), Ok(exited(0)));
        assert_eq!(
            both(Err::<ExitStatus, _>("ran".into()), Ok::<(), String>(())),
            Err("ran".into())
        );
        assert_eq!(both(ok(), Err::<(), _>("back".into())), Err("back".into()));
        assert_eq!(
            both(
                Err::<ExitStatus, _>("ran".into()),
                Err::<(), _>("back".into())
            ),
            Err("ran, and back".into())
        );
    }

    #[test]
    fn exit_codes_follow_the_shell() {
        assert_eq!(exit_code(exited(3)), ExitCode::from(3));
        #[cfg(unix)]
        assert_eq!(
            exit_code(shep_kelpie_test_support::signalled(15)),
            ExitCode::from(143)
        );
        assert_eq!(exit_code_of(Caught::Interrupt), ExitCode::from(130));
        assert_eq!(exit_code_of(Caught::Hangup), ExitCode::from(129));
    }

    // The signal lands after the handler exists and before the command
    // does, which is the gap between taking a lease and spawning. A
    // current-thread runtime runs its signal driver only when the test
    // yields, so the stream has not seen the signal when the run starts.
    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn an_interrupt_before_the_command_starts_ends_the_run_without_it() {
        let _turn = LISTENING.lock().await;
        let temp = tempfile::tempdir().unwrap();
        let marker = temp.path().join("ran");
        let mut signals = listen().unwrap();
        std::process::Command::new("kill")
            .args(["-INT", &std::process::id().to_string()])
            .status()
            .unwrap();
        // `kill` returns once the signal is queued, and under load the OS
        // handler can run on another thread a moment later. Sleeping here
        // without yielding lets it run while the runtime's driver stays idle.
        std::thread::sleep(std::time::Duration::from_millis(200));
        let command = ["touch", marker.to_str().unwrap()];
        let ended = run_command(&command, &mut signals).await;
        assert_eq!(ended, Ok(ExitCode::from(130)));
        assert!(!marker.exists(), "the command started anyway");
    }

    #[tokio::test]
    async fn the_gpu_lease_is_refused_and_paddock_is_named() {
        for args in [
            &["take", "gpu"][..],
            &["take", "gpu", "--model", "qwen3"],
            &["run", "gpu", "--", "true"],
            &["run", "gpu", "--model", "qwen3", "--", "true"],
            &["return", "gpu"],
        ] {
            let refused = dispatch(&words(args)).await.unwrap_err();
            assert!(refused.contains("shep paddock run"), "{args:?}: {refused}");
        }
    }
}
