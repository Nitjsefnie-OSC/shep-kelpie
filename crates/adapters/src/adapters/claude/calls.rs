//! A call's output and its record on disk, seen through `ClaudeCli::run`
//! with a stand-in `claude` script

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::adapters::ClaudeCli;
use crate::ports::{
    AgentCall, AgentError, AgentReply, Agents, Ending, Flying, Role, Session, SessionId, Timestamp,
    Tools,
};
use crate::settings::Effort;
use crate::test::OpenSandbox;

const RESULT: &str = include_str!("../../../../../fixtures/claude-p-result.json");

// More than a pipe holds, as a worker's long answer is
const LONG: usize = 200 * 1024;

struct World {
    dir: tempfile::TempDir,
}

impl World {
    fn new() -> Self {
        let w = Self {
            dir: crate::test::short_tempdir(),
        };
        for folder in ["home", "wt", "worker"] {
            std::fs::create_dir_all(w.path(folder)).unwrap();
        }
        std::fs::write(w.path("result.json"), RESULT).unwrap();
        w
    }

    // A stand-in `claude` that runs `body`, which names the world's files
    // through `{}` for the folder, and then prints the result in `result.json`.
    fn script(self, body: &str) -> Self {
        let body = body.replace("{}", &self.path("").display().to_string());
        let script = format!(
            "#!/bin/sh\n{body}\ncat {}\n",
            self.path("result.json").display()
        );
        crate::test::write_script(&self.path("claude"), &script);
        self
    }

    fn path(&self, p: &str) -> PathBuf {
        self.dir.path().canonicalize().unwrap().join(p)
    }

    fn cli(&self) -> ClaudeCli {
        ClaudeCli::with_program(self.path("claude"))
            .sandboxed(Arc::new(OpenSandbox::default()), self.path("home"))
    }

    fn call(&self) -> AgentCall {
        AgentCall {
            issue: crate::ticket::TicketId::from_issue(7),
            settings: self.path("worker/settings.json"),
            ..AgentCall::new(
                Role::Worker,
                Tools::Answer,
                crate::settings::AgentHarness::ClaudeCode,
                "claude-sonnet-5",
                Effort::Medium,
                Session::New(SessionId("s".into())),
                self.path("wt"),
                "go",
            )
        }
    }

    fn record(&self) -> PathBuf {
        self.path("worker/settings.call.json")
    }

    // Runs the call on a thread of its own, as the runner does
    fn start(&self, ending: &Ending) -> mpsc::Receiver<Result<AgentReply, AgentError>> {
        let (cli, call, ending) = (self.cli(), self.call(), ending.clone());
        let (done, ended) = mpsc::channel();
        thread::spawn(move || {
            cli.prepare(&call).unwrap();
            let _ = done.send(cli.run(&call, &ending));
        });
        ended
    }
}

fn wait_for(what: &str, ready: impl Fn() -> bool) {
    let started = Instant::now();
    while !ready() {
        assert!(started.elapsed() < Duration::from_secs(10), "no {what}");
        thread::sleep(Duration::from_millis(20));
    }
}

fn read(path: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn a_call_s_output_reaches_its_result_through_files() {
    // The stand-in prints a result whose text is longer than a pipe holds,
    // and notes whether its stdout is a file.
    let w = World::new().script("[ -f /dev/fd/1 ] && echo file > {}kind");
    let mut long: serde_json::Value = serde_json::from_str(RESULT).unwrap();
    long["result"] = "x".repeat(LONG).into();
    std::fs::write(w.path("result.json"), long.to_string()).unwrap();

    let call = w.call();
    let cli = w.cli();
    cli.prepare(&call).unwrap();
    let reply = cli.run(&call, &Ending::default()).unwrap();

    assert_eq!(reply.text.len(), LONG);
    assert_eq!(std::fs::read_to_string(w.path("kind")).unwrap(), "file\n");
}

#[test]
fn a_running_call_has_a_record_and_an_ended_call_has_none() {
    let w = World::new();
    let waiting = format!(
        "echo $$ > {{}}pid\ntouch {{}}started\n{}",
        crate::test::waits_for(&w.path("go"), crate::test::CALL_PATIENCE)
    );
    let w = w.script(&waiting);
    let ending = Ending::default();
    ending.carry(Flying {
        left: 600,
        began: Timestamp(1_000),
        again: false,
        first: true,
        compact: false,
    });
    let ended = w.start(&ending);
    wait_for("start", || w.path("started").exists());

    let pid: u32 = std::fs::read_to_string(w.path("pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let record = read(&w.record());
    assert_eq!(record["pid"], pid);
    assert_eq!(record["pgid"], pid);
    assert_eq!(record["role"], "worker");
    assert_eq!(record["issue"], "7");
    assert_eq!(record["ending"], false);
    assert_eq!(record["flying"]["left"], 600);
    assert_eq!(record["flying"]["began"], 1_000);
    assert_eq!(record["flying"]["first"], true);

    std::fs::write(w.path("go"), "").unwrap();
    ended
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
        .unwrap();
    assert!(!w.record().exists());
}

// The stand-ins below loop only while the test process lives, so a failed
// or killed run leaves none behind.
#[test]
fn a_call_stopped_by_the_runner_leaves_no_record() {
    let w = World::new()
        .script("touch {}started\nwhile kill -0 $PPID 2>/dev/null; do sleep 0.02; done");
    let cli = w.cli();
    let (call, ending) = (w.call(), Ending::default());
    let (done, ended) = mpsc::channel();
    let running = cli.clone();
    thread::spawn(move || {
        running.prepare(&call).unwrap();
        let _ = done.send(running.run(&call, &ending));
    });
    wait_for("start", || w.path("started").exists());
    assert!(w.record().exists());

    cli.stop();

    let result = ended.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(result.unwrap_err(), AgentError::Stopped);
    assert!(!w.record().exists());
}

// Real time: the stand-in ignores SIGTERM, so the ended call lives out the
// grace before SIGKILL, long enough to read its record.
#[test]
fn a_call_asked_to_end_says_so_in_its_record_and_leaves_none_once_ended() {
    let w = World::new().script(
        "trap '' TERM\ntouch {}started\nwhile kill -0 $PPID 2>/dev/null; do sleep 0.02; done",
    );
    let ending = Ending::default();
    let ended = w.start(&ending);
    wait_for("start", || w.path("started").exists());
    assert_eq!(read(&w.record())["ending"], false);

    ending.end();
    wait_for("ending in the record", || {
        read(&w.record())["ending"] == true
    });

    let result = ended.recv_timeout(Duration::from_secs(20)).unwrap();
    assert!(matches!(result, Err(AgentError::TimedOut(_))), "{result:?}");
    assert!(!w.record().exists());
}
