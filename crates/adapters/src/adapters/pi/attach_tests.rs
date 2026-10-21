// A pi worker's session in the maintainer's terminal: the worker's sandbox,
// tools and guard, with the maintainer at the prompt.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::Arc;

use super::tests::{URL, WORKER_ID, World, id};
use super::*;
use crate::test::{Answer, OpenSandbox, StandInEndpoint};

fn pi_in(world: &World, sandbox: &OpenSandbox) -> PiCli {
    PiCli::on(&Base::default().sandboxed(Arc::new(sandbox.clone()), world.path("home")))
}

// A worker's session pi has written, resumed with nothing to say first.
fn resumable(world: &World, url: &str) -> AgentCall {
    let mut call = world.fenced(url, Path::new("/k/kelpie"), Session::Resume(id(WORKER_ID)));
    call.instructions = Some(PathBuf::from("/k/worker/instructions.md"));
    let sessions = world.path("settings.pi/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(
        sessions.join(format!("2026-10-01T03-00-00-000Z_{WORKER_ID}.jsonl")),
        "{}\n",
    )
    .unwrap();
    call
}

fn args_of(command: &Command) -> Vec<String> {
    command
        .get_args()
        .map(|a| a.to_str().unwrap().to_owned())
        .collect()
}

// The policy handed to the sandbox, less the socket its forwarder got.
fn policy_without_socket(sandbox: &OpenSandbox, at: usize) -> Policy {
    let mut policy = sandbox.wrapped()[at].0.clone();
    policy.forward = None;
    policy
}

#[test]
fn a_pi_session_for_the_terminal_resumes_in_the_workers_sandbox() {
    let w = World::new();
    let sandbox = OpenSandbox::default();
    let pi = pi_in(&w, &sandbox);
    let call = resumable(&w, URL);
    pi.prepare(&call).unwrap();

    let command = pi.foreground(&call).unwrap();

    let args = args_of(&command);
    let at = args.iter().position(|a| a == "pi").expect("pi is run");
    let home = w.path("settings.pi");
    assert_eq!(
        args[at..],
        [
            "pi",
            "--model",
            "kelpie/qwen3.8:27b",
            "--thinking",
            "low",
            "--no-extensions",
            "--no-skills",
            "--no-prompt-templates",
            "--no-themes",
            "--no-approve",
            "--offline",
            "--session-dir",
            home.join("sessions").to_str().unwrap(),
            "--tools",
            "read,bash,edit,write,grep,find,ls",
            "--no-mcp",
            "--append-system-prompt",
            "/k/worker/instructions.md",
            "--extension",
            w.path("settings.guard.ts").to_str().unwrap(),
            "--session",
            WORKER_ID,
        ]
    );
    for headless in ["-p", "--mode", "--session-id"] {
        assert!(!args.iter().any(|a| a == headless), "{headless}: {args:?}");
    }
    let door = format!("{}=/k/dog/lease.sock", crate::lease::door::SOCKET_VAR);
    for var in [
        format!("PI_CODING_AGENT_DIR={}", home.display()),
        format!("TMPDIR={}", w.path("settings.tmp").display()),
        door,
    ] {
        assert!(args[..at].contains(&var), "{var}: {args:?}");
    }
    assert_eq!(command.get_current_dir(), Some(w.path("wt").as_path()));

    let (_headless, _forwarder) = pi.sandboxed_command(&call, None).unwrap();
    assert_eq!(
        policy_without_socket(&sandbox, 0),
        policy_without_socket(&sandbox, 1),
        "the terminal is held to the headless call's policy"
    );
}

#[test]
fn a_non_empty_prompt_is_the_first_message() {
    let w = World::new();
    let pi = pi_in(&w, &OpenSandbox::default());
    let mut call = resumable(&w, URL);
    call.prompt = "pick up where you left off".into();
    pi.prepare(&call).unwrap();

    let args = args_of(&pi.foreground(&call).unwrap());

    assert_eq!(
        args[args.len() - 4..],
        ["--session", WORKER_ID, "--", "pick up where you left off"]
    );
}

#[test]
fn a_session_pi_never_wrote_is_refused() {
    let w = World::new();
    let pi = pi_in(&w, &OpenSandbox::default());
    let call = w.fenced(URL, Path::new("/k/kelpie"), Session::Resume(id(WORKER_ID)));
    pi.prepare(&call).unwrap();

    let err = pi.foreground(&call).unwrap_err();

    assert_eq!(err, AgentError::NoSession(Harness::Pi, id(WORKER_ID)));
}

#[test]
fn the_terminal_session_has_a_forwarder_until_foreground_ended() {
    let server = StandInEndpoint::start([Answer::Says("hello")]);
    let w = World::new();
    let sandbox = OpenSandbox::default();
    let pi = pi_in(&w, &sandbox);
    let call = resumable(&w, server.url());
    pi.prepare(&call).unwrap();

    pi.foreground(&call).unwrap();

    let socket = sandbox.wrapped()[0].0.forward.clone().unwrap().socket;
    let body = r#"{"model":"qwen3.8:27b"}"#;
    let chat = format!(
        "POST http://model.kelpie.test/v1/chat/completions HTTP/1.1\r\n\
         content-length: {}\r\n\r\n{body}",
        body.len()
    );
    let mut stream = UnixStream::connect(&socket).unwrap();
    stream.write_all(chat.as_bytes()).unwrap();
    let mut reply = String::new();
    stream.read_to_string(&mut reply).unwrap();
    assert!(reply.contains("hello"), "{reply}");

    pi.foreground_ended(call.session.id());
    assert!(!socket.exists(), "the socket is removed");
}
