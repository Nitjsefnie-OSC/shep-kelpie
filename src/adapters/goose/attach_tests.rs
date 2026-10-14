// A goose worker's session in the maintainer's terminal: the worker's sandbox,
// env and hook, with the maintainer at the prompt.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::Arc;

use super::tests::{FRESH_ID, OTHER_ID, RESUMED, URL, World, id, output};
use super::*;
use crate::ports::Usage;
use crate::test::{Answer, OpenSandbox, StandInEndpoint};

fn goose_in(world: &World, sandbox: &OpenSandbox) -> GooseCli {
    ClaudeCli::default()
        .sandboxed(Arc::new(sandbox.clone()), world.path("home"))
        .goose()
}

// A worker's session goose has written, to be resumed with nothing to say first.
fn resumable(world: &World, url: &str) -> AgentCall {
    let mut call = world.fenced(url, Path::new("/k/kelpie"), Session::Resume(id(FRESH_ID)));
    call.prompt = String::new();
    Files::of(&call.settings)
        .keep(call.session.id(), Spent::default())
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
fn a_goose_session_for_the_terminal_resumes_in_the_workers_sandbox() {
    let w = World::new();
    let sandbox = OpenSandbox::default();
    let goose = goose_in(&w, &sandbox);
    let call = resumable(&w, URL);
    goose.prepare(&call).unwrap();

    let command = goose.foreground(&call).unwrap();

    let args = args_of(&command);
    let at = args
        .iter()
        .position(|a| a == "goose")
        .expect("goose is run");
    assert_eq!(
        args[at..],
        ["goose", "session", "--resume", "--name", FRESH_ID]
    );
    let root = w.path("settings.goose");
    for var in [
        format!("GOOSE_PATH_ROOT={}", root.display()),
        "GOOSE_DISABLE_KEYRING=1".into(),
        "GOOSE_TELEMETRY_OFF=1".into(),
        "GOOSE_PROVIDER=openai".into(),
        "GOOSE_MODEL=qwen3.8:27b".into(),
        "GOOSE_MODE=approve".into(),
    ] {
        assert!(args[..at].contains(&var), "{var}: {args:?}");
    }
    assert!(!args[..at].iter().any(|a| a == "GOOSE_MODE=auto"));
    assert_eq!(command.get_current_dir(), Some(w.path("wt").as_path()));

    let (_headless, _forwarder) = goose.sandboxed_command(&call, None).unwrap();
    assert_eq!(
        policy_without_socket(&sandbox, 0),
        policy_without_socket(&sandbox, 1),
        "the terminal is held to the headless call's policy"
    );
}

#[test]
fn a_non_empty_prompt_is_refused_since_goose_session_takes_no_first_message() {
    let w = World::new();
    let goose = goose_in(&w, &OpenSandbox::default());
    let mut call = resumable(&w, URL);
    call.prompt = "pick up where you left off".into();
    goose.prepare(&call).unwrap();

    let err = goose.foreground(&call).unwrap_err();

    let AgentError::Setup(why) = err else {
        panic!("{err:?}");
    };
    assert!(why.contains("first message"), "{why}");
}

#[test]
fn a_session_goose_never_wrote_is_refused() {
    let w = World::new();
    let goose = goose_in(&w, &OpenSandbox::default());
    let mut call = resumable(&w, URL);
    call.session = Session::Resume(id(OTHER_ID));
    goose.prepare(&call).unwrap();

    let err = goose.foreground(&call).unwrap_err();

    assert_eq!(err, AgentError::NoSession(Harness::Goose, id(OTHER_ID)));
}

#[test]
fn the_terminal_session_has_a_forwarder_until_foreground_ended() {
    let server = StandInEndpoint::start([Answer::Says("hello")]);
    let w = World::new();
    let sandbox = OpenSandbox::default();
    let goose = goose_in(&w, &sandbox);
    let call = resumable(&w, server.url());
    goose.prepare(&call).unwrap();

    goose.foreground(&call).unwrap();

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

    goose.foreground_ended(call.session.id());
    assert!(!socket.exists(), "the socket is removed");
}

// What goose reports after a session the terminal spent tokens in: the totals of
// `RESUMED`, which are the headless turn's and the terminal's together.
#[test]
fn the_first_turn_after_a_terminal_session_is_charged_for_none_of_the_gap() {
    let before = Spent {
        input_tokens: 20,
        output_tokens: 3,
        rebase: true,
        ..Spent::default()
    };

    let read = output::read(
        &output(0, RESUMED, ""),
        &Session::Resume(id(FRESH_ID)),
        before,
    );

    assert_eq!(read.reply.unwrap().usage, Usage::default());
    let spent = read.spent.expect("the turn reported totals");
    assert_eq!((spent.input_tokens, spent.output_tokens), (69, 15));
    assert!(!spent.rebase, "the next turn counts from these totals");
}

#[test]
fn the_end_of_a_terminal_session_leaves_its_totals_to_be_counted_from_again() {
    let w = World::new();
    let goose = goose_in(&w, &OpenSandbox::default());
    let call = resumable(&w, URL);
    let files = Files::of(&call.settings);
    let spent = Spent {
        input_tokens: 20,
        ..Spent::default()
    };
    files.keep(call.session.id(), spent).unwrap();
    goose.prepare(&call).unwrap();
    goose.foreground(&call).unwrap();

    goose.foreground_ended(call.session.id());

    let kept = files.spent(call.session.id()).unwrap().unwrap();
    assert_eq!(
        kept,
        Spent {
            rebase: true,
            ..spent
        }
    );
}
