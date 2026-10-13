//! goose's error text read as a model host that cannot be reached

use super::output::{Spent, read};
use super::tests::{FRESH_ID, UNREACHABLE, id, output};
use super::*;

fn errored_with(message: &str) -> Result<AgentReply, AgentError> {
    let event = format!(
        r#"{{"type":"error","error":{}}}"#,
        serde_json::Value::from(message)
    );
    read(
        &output(1, &event, ""),
        &Session::New(id(FRESH_ID)),
        Spent::default(),
    )
    .reply
}

#[test]
fn a_model_host_that_cannot_be_reached_is_unreachable_and_any_other_error_fails() {
    for message in [
        r#"502 "unreachable""#,
        "503 Service Unavailable",
        "Request failed: error sending request: Connection refused (os error 61)",
        "read ECONNRESET",
        "Network error: Could not connect to 192.0.2.9:11434",
        "192.0.2.9:11434: connection refused",
    ] {
        assert_eq!(
            errored_with(message),
            Err(AgentError::Unreachable(Harness::Goose, message.into())),
            "{message}"
        );
    }

    // goose reports a refused connection as its reply and exits 0, with no
    // tokens spent.
    let parsed = read(
        &output(0, UNREACHABLE, ""),
        &Session::New(id(FRESH_ID)),
        Spent::default(),
    );
    let err = parsed.reply.unwrap_err();
    assert!(
        matches!(&err, AgentError::Unreachable(Harness::Goose, why) if why.contains("127.0.0.1:9")),
        "{err:?}"
    );

    // A refused connection is also what a goose that exits non-zero prints.
    let err = read(
        &output(1, "", "Error: connection refused"),
        &Session::New(id(FRESH_ID)),
        Spent::default(),
    )
    .reply
    .unwrap_err();
    assert!(
        matches!(err, AgentError::Unreachable(Harness::Goose, _)),
        "{err:?}"
    );

    let message = r#"400 "context length exceeded""#;
    assert_eq!(
        errored_with(message),
        Err(AgentError::Failed(Harness::Goose, message.into()))
    );
    // An error that leads with another status is the call's own, even when
    // its text mentions a refused or reset connection.
    for message in [
        r#"400 "upstream connection refused""#,
        "429 read ECONNRESET while retrying",
    ] {
        assert_eq!(
            errored_with(message),
            Err(AgentError::Failed(Harness::Goose, message.into())),
            "{message}"
        );
    }
    // A status number inside other text is not the host's status.
    let message = "the model said 502 things";
    assert_eq!(
        errored_with(message),
        Err(AgentError::Failed(Harness::Goose, message.into()))
    );

    // The sign may come before the last 2048 bytes an error keeps.
    let long = format!("connection refused {}", "x".repeat(4096));
    let err = errored_with(&long).unwrap_err();
    assert!(
        matches!(&err, AgentError::Unreachable(Harness::Goose, why) if why.len() == 2048),
        "{err:?}"
    );

    // A reply that begins "Network error:" but spent tokens is the model's own words.
    let spent = UNREACHABLE.replace(r#""input_tokens":0"#, r#""input_tokens":5"#);
    let reply = read(
        &output(0, &spent, ""),
        &Session::New(id(FRESH_ID)),
        Spent::default(),
    )
    .reply;
    assert!(reply.is_ok(), "{reply:?}");
}

#[test]
fn a_closed_port_on_a_resumed_session_is_unreachable_and_keeps_its_totals() {
    let before = Spent {
        input_tokens: 20,
        output_tokens: 3,
        ..Spent::default()
    };
    let parsed = read(
        &output(0, UNREACHABLE, ""),
        &Session::Resume(id(FRESH_ID)),
        before,
    );
    let err = parsed.reply.unwrap_err();
    assert!(
        matches!(err, AgentError::Unreachable(Harness::Goose, _)),
        "{err:?}"
    );
    assert_eq!(parsed.spent, None);
}

#[test]
fn a_reply_streamed_in_pieces_is_joined_by_the_id_they_share() {
    let piece = |id: &str, text: &str| {
        format!(
            r#"{{"type":"message","message":{{"id":"{id}","role":"assistant","content":[{{"type":"text","text":"{text}"}}]}}}}"#
        )
    };
    let stdout = [
        piece("a", "Earlier. "),
        piece("b", "Hello "),
        piece("b", "world"),
    ]
    .join("\n");
    let reply = read(
        &output(0, &stdout, ""),
        &Session::New(id(FRESH_ID)),
        Spent::default(),
    )
    .reply
    .unwrap();
    assert_eq!(reply.text, "Hello world");
}
