use std::collections::BTreeMap;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

use super::*;
use crate::ports::Policy;
use crate::test::{OpenSandbox, short_tempdir, write_script};

fn servers(sandbox: &OpenSandbox) -> SandboxedServers {
    SandboxedServers {
        sandbox: Arc::new(sandbox.clone()),
    }
}

// A run of `script` in `dir`, which answers on `port` if it is that port's listener.
fn run_of(dir: &Path, script: &str, port: u16, wait: Duration) -> DevRun {
    let program = dir.join("serve");
    write_script(&program, script);
    DevRun {
        program: program.display().to_string(),
        args: vec!["--port".into(), port.to_string()],
        cwd: dir.to_owned(),
        port,
        env: BTreeMap::from([("PORT".to_owned(), port.to_string())]),
        policy: Policy {
            listen: true,
            ..Policy::default()
        },
        settings: dir.join("dev.sandbox.json"),
        log: dir.join("dev-server.log"),
        ready_within: wait,
        gate: crate::ports::DevPort::default(),
    }
}

// A port nothing listens on: bound and let go.
fn unused() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

// A script that listens on its port, as a dev server does, and says so in its log.
const LISTENER: &str = "#!/bin/sh\necho \"serving $PORT\"\nexec python3 -c 'import socket, sys, time; \
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); \
s.bind((\"127.0.0.1\", int(sys.argv[1]))); s.listen(); time.sleep(30)' \"$PORT\"\n";

#[test]
fn a_server_that_binds_its_port_is_returned_answering_and_runs_in_its_sandbox() {
    let dir = short_tempdir();
    let port = unused();
    let sandbox = OpenSandbox::default();
    let run = run_of(dir.path(), LISTENER, port, Duration::from_secs(10));
    let servers = servers(&sandbox);
    assert!(servers.free(port), "nothing listens before the start");
    let started = servers.start(&run).unwrap();
    assert!(started.answered);
    let [(policy, settings)] = sandbox.wrapped().try_into().unwrap();
    assert!(policy.listen);
    assert_eq!(settings, run.settings);
    drop(started);
    let log = std::fs::read_to_string(&run.log).unwrap();
    assert!(log.contains(&format!("serving {port}")), "{log}");
}

#[test]
fn a_port_that_something_else_took_starts_nothing() {
    let dir = short_tempdir();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let run = run_of(
        dir.path(),
        "#!/bin/sh\ntouch ran\n",
        port,
        Duration::from_secs(10),
    );
    let why = servers(&OpenSandbox::default()).start(&run).unwrap_err();
    assert!(why.contains(&format!("port {port} is taken")), "{why}");
    assert!(!dir.path().join("ran").exists());
}

#[test]
fn a_server_that_never_answers_is_returned_after_its_wait_and_still_runs_until_dropped() {
    let dir = short_tempdir();
    let run = run_of(
        dir.path(),
        "#!/bin/sh\nexec sleep 30\n",
        unused(),
        Duration::from_millis(600),
    );
    let began = Instant::now();
    let started = servers(&OpenSandbox::default()).start(&run).unwrap();
    assert!(!started.answered);
    assert!(began.elapsed() >= Duration::from_millis(600));
    drop(started);
}

#[test]
fn a_server_that_exits_is_not_waited_on() {
    let dir = short_tempdir();
    let run = run_of(
        dir.path(),
        "#!/bin/sh\necho 'no such script: dev' >&2\nexit 1\n",
        unused(),
        Duration::from_secs(30),
    );
    let began = Instant::now();
    let started = servers(&OpenSandbox::default()).start(&run).unwrap();
    assert!(!started.answered);
    assert!(began.elapsed() < Duration::from_secs(10));
    let log = std::fs::read_to_string(&run.log).unwrap();
    assert!(log.contains("no such script"), "{log}");
}

#[test]
fn dropping_a_server_ends_what_it_started() {
    let dir = short_tempdir();
    let marker = dir.path().join("pid");
    let script = format!(
        "#!/bin/sh\nsleep 300 &\necho $! > {}\nwait\n",
        marker.display()
    );
    let run = run_of(dir.path(), &script, unused(), Duration::from_millis(1500));
    let started = servers(&OpenSandbox::default()).start(&run).unwrap();
    let pid = std::fs::read_to_string(&marker).unwrap().trim().to_owned();
    let alive = |pid: &str| {
        Command::new("kill")
            .args(["-0", pid])
            .output()
            .unwrap()
            .status
            .success()
    };
    assert!(alive(&pid), "the server's child was not running");
    drop(started);
    thread::sleep(Duration::from_millis(200));
    assert!(!alive(&pid), "the server's child outlived it");
}

#[test]
fn a_link_left_at_the_log_is_replaced_not_written_through() {
    let dir = short_tempdir();
    let victim = dir.path().join("victim");
    std::fs::write(&victim, "keep\n").unwrap();
    let run = run_of(
        dir.path(),
        "#!/bin/sh\necho written\n",
        unused(),
        Duration::from_millis(300),
    );
    std::os::unix::fs::symlink(&victim, &run.log).unwrap();
    let started = servers(&OpenSandbox::default()).start(&run).unwrap();
    drop(started);
    assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep\n");
    assert!(!run.log.is_symlink());
    assert!(
        std::fs::read_to_string(&run.log)
            .unwrap()
            .contains("written")
    );
}

#[test]
fn a_port_something_listens_on_is_not_free_and_a_picked_port_is() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let held = listener.local_addr().unwrap().port();
    let servers = servers(&OpenSandbox::default());
    assert!(!servers.free(held));
    let picked = servers.pick().unwrap();
    assert_ne!(picked, held);
    assert!(servers.free(picked));
}

#[test]
fn a_sandbox_that_refuses_starts_nothing() {
    #[derive(Debug)]
    struct Refusing;
    impl Sandbox for Refusing {
        fn wrap(
            &self,
            _: &Policy,
            _: &Path,
            _: &Command,
        ) -> Result<Command, crate::ports::SandboxError> {
            Err(crate::ports::SandboxError::Missing(PathBuf::from("/srt")))
        }
    }
    let dir = short_tempdir();
    let run = run_of(
        dir.path(),
        "#!/bin/sh\ntouch ran\n",
        unused(),
        Duration::from_millis(300),
    );
    let servers = SandboxedServers {
        sandbox: Arc::new(Refusing),
    };
    let why = servers.start(&run).unwrap_err();
    assert!(why.contains("sandbox runtime is missing"), "{why}");
    assert!(!dir.path().join("ran").exists());
}
