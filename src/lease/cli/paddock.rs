//! `lease take|return gpu`: the maintainer's hold on a model, as a paddock lease
//!
//! `lease run gpu --model <m> -- <command>` is `shep paddock run --model <m>
//! -- <command>`, which waits for the lease, runs the command and gives the
//! lease back when it exits. A `take` runs the same under a holder: kelpie
//! itself, started by `shep paddock run` and idle, so the lease lasts as
//! long as the holder does. A record beside it names the holder for `return`,
//! which ends it with `TERM`; `shep paddock run` passes that on and gives the
//! lease back once the holder has exited.

use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{ExitCode, Stdio};
use std::time::{Duration, Instant};

use tokio::signal::unix::{SignalKind, signal};

use super::signal_process;
use crate::lease::gpu;

/// The program that runs a paddock lease: shep's own `paddock run`
pub(super) const SHEP: &str = "shep";

/// Why a `gpu` lease is refused without a model
pub(super) const NEEDS_MODEL: &str = "the GPU lease is a paddock lease on a model: \
     add `--model <model>`, as `shep paddock run --model` takes it";

/// What a `take` holder writes as its lease's note, so paddock's status says whose it is
const TAKE_NOTE: &str =
    "shep kelpie lease take, held for the maintainer until `shep kelpie lease return gpu`";

/// The line a holder prints once its lease is granted
const HELD: &str = "held";

/// How long `return` waits for the holder to exit, and how often it looks
const ENDS_WITHIN: Duration = Duration::from_secs(5);
const LOOK_EVERY: Duration = Duration::from_millis(50);

/// The command line that holds `model` around `command`, noted as `note`
pub(super) fn run_line<'a>(
    shep: &'a str,
    model: &'a str,
    note: &'a str,
    command: &[&'a str],
) -> Vec<&'a str> {
    let mut line = vec![
        shep, "paddock", "run", "--model", model, "--note", note, "--",
    ];
    line.extend_from_slice(command);
    line
}

/// The note a `run` leaves on its lease
pub(super) fn run_note(command: &[&str]) -> String {
    format!("shep kelpie lease run: {}", command.join(" "))
}

// Where the record of a `take` is: the holder's pid and its model.
fn folder() -> PathBuf {
    gpu::temp_dir().join("kelpie-paddock")
}

struct Taken {
    pid: u32,
    model: String,
}

fn read_record() -> Option<Taken> {
    let text = fs::read_to_string(folder().join("take")).ok()?;
    let mut lines = text.lines();
    let pid = lines.next()?.parse().ok()?;
    Some(Taken {
        pid,
        model: lines.next()?.to_owned(),
    })
}

/// Holds `model` for the maintainer until `return`, and says so
pub(super) fn take(shep: &str, model: &str) -> Result<(), String> {
    if let Some(taken) = read_record().filter(|t| gpu::alive(t.pid)) {
        println!(
            "shep kelpie lease: the GPU lease on {} is already held for you by pid {}",
            taken.model, taken.pid
        );
        return Ok(());
    }
    let folder = folder();
    fs::create_dir_all(&folder).map_err(|e| format!("cannot make {}: {e}", folder.display()))?;
    // The holder outlives take, so it keeps none of take's own output
    // open: a caller reading take's output to its end would wait forever.
    // What shep says goes to a file, since nothing reads it after take.
    let log = folder.join("take.log");
    let said =
        fs::File::create(&log).map_err(|e| format!("cannot write {}: {e}", log.display()))?;
    let exe = std::env::current_exe().map_err(|e| format!("cannot find kelpie itself: {e}"))?;
    let exe = exe.to_string_lossy();
    let line = run_line(shep, model, TAKE_NOTE, &[&exe, "lease", "hold", "gpu"]);
    let (program, args) = line.split_first().ok_or("no command to hold with")?;
    let mut holder = crate::spawn::command(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(said)
        .spawn()
        .map_err(|e| format!("cannot run {program}: {e}"))?;
    let pid = holder.id();
    let stdout = holder.stdout.take().ok_or("the holder has no stdout")?;
    let mut held = false;
    for line in BufReader::new(stdout).lines() {
        let line = line.map_err(|e| format!("cannot read the holder: {e}"))?;
        if line == HELD {
            held = true;
            break;
        }
        eprintln!("{line}");
    }
    if !held {
        let status = holder.wait().map_err(|e| e.to_string())?;
        let why = fs::read_to_string(&log).unwrap_or_default();
        return Err(format!("the holder gave up ({status}): {}", why.trim()));
    }
    fs::write(folder.join("take"), format!("{pid}\n{model}\n"))
        .map_err(|e| format!("cannot record the holder, so `return` could not find it: {e}"))?;
    println!(
        "shep kelpie lease: pid {pid} holds the GPU lease on {model} until `shep kelpie lease return gpu`"
    );
    Ok(())
}

/// The holder behind `take`, run by `shep paddock run`: it prints [`HELD`]
/// and lives until `return` ends it, ignoring the hangup a closed terminal
/// sends.
pub(super) async fn hold() -> Result<ExitCode, String> {
    let _hangups = signal(SignalKind::hangup()).map_err(|e| e.to_string())?;
    let mut stdout = std::io::stdout();
    // A take that is gone cannot hear HELD, and nobody would return the lease.
    writeln!(stdout, "{HELD}")
        .and_then(|()| stdout.flush())
        .map_err(|e| format!("take went away, so the lease is let go: {e}"))?;
    std::future::pending().await
}

/// Gives back what `take` holds
pub(super) fn give_back() -> Result<(), String> {
    let Some(taken) = read_record() else {
        return Err("nothing was taken: there is no GPU lease to return".into());
    };
    let record = folder().join("take");
    let removed = |record: &PathBuf| {
        fs::remove_file(record).map_err(|e| format!("cannot remove {}: {e}", record.display()))
    };
    if !gpu::alive(taken.pid) {
        removed(&record)?;
        return Err(format!(
            "the holder, pid {}, is gone, and paddock took its lease on {} back with it",
            taken.pid, taken.model
        ));
    }
    removed(&record)?;
    signal_process(taken.pid, "TERM");
    let by = Instant::now() + ENDS_WITHIN;
    while gpu::alive(taken.pid) {
        if Instant::now() >= by {
            eprintln!(
                "shep kelpie lease: pid {} has not exited yet, and paddock gives the lease back once it has",
                taken.pid
            );
            break;
        }
        std::thread::sleep(LOOK_EVERY);
    }
    println!(
        "shep kelpie lease: returned the GPU lease on {}",
        taken.model
    );
    Ok(())
}
