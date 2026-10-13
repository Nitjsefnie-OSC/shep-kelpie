//! `shep kelpie lease` on the GPU, end to end against the real binary
//!
//! The GPU lease is a paddock lease: kelpie runs `shep paddock run`. Every
//! test puts a stand-in `shep` first on its `PATH`, which notes its
//! arguments and runs the command after `--` as paddock would, under its own
//! scratch `TMPDIR`, never the maintainer's.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

const KELPIE: &str = env!("CARGO_BIN_EXE_shep-kelpie");
const TAKE_NOTE: &str =
    "shep kelpie lease take, held for the maintainer until `shep kelpie lease return gpu`";

// What a `paddock run` does, less the lease: notes its arguments, one a line,
// and runs the command after `--`.
const RUNS_THE_COMMAND: &str = "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$(dirname \"$0\")/../shep.args\"\n\
     while [ \"$1\" != -- ]; do shift; done\nshift\nexec \"$@\"\n";

// A scratch temporary folder and no shepherd, so no test reaches the
// maintainer's lock or dog.
struct Scratch {
    temp: tempfile::TempDir,
}

impl Scratch {
    fn new() -> Self {
        Self {
            temp: tempfile::tempdir().unwrap(),
        }
    }

    fn lock(&self) -> PathBuf {
        self.temp.path().join("qwen-review/gpu.lock")
    }

    // Puts a `shep` with this script first on the PATH kelpie gets.
    fn shep(&self, script: &str) {
        let bin = self.temp.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let shep = bin.join("shep");
        std::fs::write(&shep, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shep, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    // What the stand-in `shep` was last run with, one argument a line.
    fn shep_args(&self) -> Option<Vec<String>> {
        let seen = std::fs::read_to_string(self.temp.path().join("shep.args")).ok()?;
        Some(seen.lines().map(str::to_owned).collect())
    }

    fn take_record(&self) -> Option<(u32, String)> {
        let path = self.temp.path().join("kelpie-paddock/take");
        let text = std::fs::read_to_string(path).ok()?;
        let (pid, model) = text.split_once('\n')?;
        Some((pid.parse().ok()?, model.trim().to_owned()))
    }

    fn kelpie(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(KELPIE);
        let path = std::env::var("PATH").unwrap_or_default();
        let bin = self.temp.path().join("bin");
        cmd.args(args)
            .env("TMPDIR", self.temp.path())
            .env("PATH", format!("{}:{path}", bin.display()))
            .env("SHEP_HOME", self.temp.path().join("no-shepherd"))
            .stdin(Stdio::null());
        cmd
    }
}

fn until(what: &str, timeout: Duration, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + timeout;
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

fn wait_output(child: Child, timeout: Duration) -> Output {
    let pid = child.id();
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || done.send(child.wait_with_output().unwrap()));
    match finished.recv_timeout(timeout) {
        Ok(output) => output,
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            let _ = Command::new("kill")
                .args(["-KILL", &pid.to_string()])
                .status();
            panic!("pid {pid} did not exit within {timeout:?}, so it was killed")
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            panic!("waiting on pid {pid} failed")
        }
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[test]
fn run_holds_a_paddock_lease_on_the_model_while_the_command_runs() {
    let s = Scratch::new();
    s.shep(RUNS_THE_COMMAND);
    let out = s
        .kelpie(&[
            "lease",
            "run",
            "gpu",
            "--model",
            "qwen3",
            "--",
            "sh",
            "-c",
            "echo ran; exit 3",
        ])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "ran\n");
    assert_eq!(
        s.shep_args().unwrap(),
        [
            "paddock",
            "run",
            "--model",
            "qwen3",
            "--note",
            "shep kelpie lease run: sh -c echo ran; exit 3",
            "--",
            "sh",
            "-c",
            "echo ran; exit 3",
        ]
    );
    assert!(!s.lock().exists(), "the gpu lock has no part in it");
}

#[test]
fn run_without_a_model_is_refused() {
    let s = Scratch::new();
    s.shep(RUNS_THE_COMMAND);
    let out = s
        .kelpie(&["lease", "run", "gpu", "--", "true"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("add `--model <model>`"),
        "{}",
        text(&out.stderr)
    );
    assert_eq!(s.shep_args(), None, "paddock ran");
}

#[test]
fn run_stops_on_an_interrupt_and_passes_a_terminate_on() {
    let s = Scratch::new();
    s.shep(RUNS_THE_COMMAND);
    // Its own process group, so the interrupt reaches kelpie and the
    // command together, as a terminal's does.
    // The command says when it has started, since an interrupt before that
    // reaches only kelpie, and the unit test in `lease/cli.rs` covers it.
    let started = s.temp.path().join("started");
    let script = r#"touch "$0"; exec sleep 30"#;
    let run = |s: &Scratch| {
        use std::os::unix::process::CommandExt;
        s.kelpie(&[
            "lease",
            "run",
            "gpu",
            "--model",
            "qwen3",
            "--",
            "sh",
            "-c",
            script,
            started.to_str().unwrap(),
        ])
        .process_group(0)
        .stderr(Stdio::piped())
        .spawn()
        .unwrap()
    };
    let child = run(&s);
    until("the command", Duration::from_secs(5), || started.exists());
    Command::new("kill")
        .args(["-INT", "--", &format!("-{}", child.id())])
        .status()
        .unwrap();
    let out = wait_output(child, Duration::from_secs(5));
    assert_eq!(out.status.code(), Some(130), "{}", text(&out.stderr));

    std::fs::remove_file(&started).unwrap();
    let child = run(&s);
    until("the command", Duration::from_secs(5), || started.exists());
    Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    let out = wait_output(child, Duration::from_secs(5));
    assert_eq!(out.status.code(), Some(143), "{}", text(&out.stderr));
}

#[test]
fn run_without_shep_says_it_cannot_run_it() {
    let s = Scratch::new();
    let out = s
        .kelpie(&["lease", "run", "gpu", "--model", "qwen3", "--", "true"])
        .env("PATH", "/no/such/bin")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stderr).contains("cannot run shep"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn take_holds_the_lease_past_its_own_exit_until_return() {
    let s = Scratch::new();
    s.shep(RUNS_THE_COMMAND);
    let take = s
        .kelpie(&["lease", "take", "gpu", "--model", "qwen3"])
        .output()
        .unwrap();
    assert!(take.status.success(), "{}", text(&take.stderr));
    let (holder, model) = s.take_record().expect("take records its holder");
    assert_eq!(model, "qwen3");
    assert!(alive(holder), "the holder died with take");
    let args = s.shep_args().unwrap();
    assert_eq!(
        args[..7],
        [
            "paddock", "run", "--model", "qwen3", "--note", TAKE_NOTE, "--"
        ]
    );
    assert_eq!(args[8..], ["lease", "hold", "gpu"]);

    let again = s
        .kelpie(&["lease", "take", "gpu", "--model", "qwen3"])
        .output()
        .unwrap();
    assert!(
        text(&again.stdout).contains("already held for you"),
        "{}",
        text(&again.stdout)
    );

    let back = s.kelpie(&["lease", "return", "gpu"]).output().unwrap();
    assert!(back.status.success(), "{}", text(&back.stderr));
    assert!(!alive(holder), "return left the holder running");
    assert_eq!(s.take_record(), None);
    assert!(!s.lock().exists(), "the gpu lock has no part in it");

    let twice = s.kelpie(&["lease", "return", "gpu"]).output().unwrap();
    assert_eq!(twice.status.code(), Some(1));
    assert!(text(&twice.stderr).contains("there is no GPU lease to return"));
}

#[test]
fn take_without_a_model_is_refused() {
    let s = Scratch::new();
    s.shep(RUNS_THE_COMMAND);
    let out = s.kelpie(&["lease", "take", "gpu"]).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(text(&out.stderr).contains("add `--model <model>`"));
    assert_eq!(s.shep_args(), None, "paddock ran");
}

#[test]
fn take_says_why_when_paddock_refuses_the_lease() {
    let s = Scratch::new();
    s.shep("#!/bin/sh\necho 'paddock: no such model qwen3' >&2\nexit 75\n");
    let out = s
        .kelpie(&["lease", "take", "gpu", "--model", "qwen3"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let said = text(&out.stderr);
    assert!(said.contains("the holder gave up"), "{said}");
    assert!(said.contains("no such model qwen3"), "{said}");
    assert_eq!(s.take_record(), None);
}

#[test]
fn status_reads_the_gpu_from_its_lock_when_no_dog_answers() {
    let s = Scratch::new();
    let mut round = Command::new("sleep").arg("30").spawn().unwrap();
    std::fs::create_dir_all(s.lock()).unwrap();
    std::fs::write(s.lock().join("pid"), format!("{}\n", round.id())).unwrap();
    std::fs::write(s.lock().join("what"), "round 2 in /tmp/hunks\n").unwrap();

    let out = s.kelpie(&["lease", "status"]).output().unwrap();
    round.kill().unwrap();
    round.wait().unwrap();
    assert_eq!(out.status.code(), Some(1), "no dog is running");
    assert!(text(&out.stderr).contains("the dog did not answer"));
    let status: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let gpu = &status["leases"][0];
    assert_eq!(gpu["kind"], "gpu");
    assert_eq!(gpu["lock"], s.lock().display().to_string());
    assert_eq!(gpu["holder"]["pid"], round.id());
    assert_eq!(gpu["holder"]["what"], "round 2 in /tmp/hunks");
    assert!(gpu["since"].as_u64().is_some(), "{gpu}");
}

// A sheep's environment: what the pinned shepherd hands its sheep.
fn lock_seen_by(env: &[(&str, &Path)]) -> String {
    let scratch = tempfile::tempdir().unwrap();
    let mut cmd = Command::new(KELPIE);
    cmd.args(["lease", "status"])
        .env_clear()
        .env("HOME", std::env::var_os("HOME").unwrap())
        .env("PATH", std::env::var_os("PATH").unwrap())
        .env("SHEP_HOME", scratch.path());
    for (name, value) in env {
        cmd.env(name, value);
    }
    let out = cmd.output().unwrap();
    let status: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    status["leases"][0]["lock"].as_str().unwrap().to_owned()
}

#[test]
#[cfg(target_os = "macos")]
fn a_sheep_without_tmpdir_finds_the_lock_the_maintainers_shell_uses() {
    let getconf = Command::new("getconf")
        .arg("DARWIN_USER_TEMP_DIR")
        .output()
        .unwrap();
    let user_temp = text(&getconf.stdout).trim().to_owned();
    assert_eq!(
        Path::new(&lock_seen_by(&[])),
        Path::new(&user_temp).join("qwen-review/gpu.lock")
    );
}

#[test]
fn a_sheep_given_tmpdir_uses_it() {
    let scratch = tempfile::tempdir().unwrap();
    assert_eq!(
        Path::new(&lock_seen_by(&[("TMPDIR", scratch.path())])),
        scratch.path().join("qwen-review/gpu.lock")
    );
}

#[test]
fn an_unknown_lease_command_prints_usage() {
    let s = Scratch::new();
    let out = s.kelpie(&["lease", "run", "gpu", "--"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(text(&out.stderr).contains("shep-kelpie lease take <kind>"));
}
