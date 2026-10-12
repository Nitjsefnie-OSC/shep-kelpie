//! pi's error text read as a model host that cannot be reached

use super::tests::{FRESH, FRESH_ID, id, output};
use super::*;

// pi ends a turn whose request failed with `stopReason` "error" and the
// SDK's message, so a model host that cannot be reached reads as these.
fn errored_with(message: &str) -> Result<AgentReply, AgentError> {
    let text = FRESH.replace(
        r#""stopReason":"stop""#,
        &format!(
            r#""stopReason":"error","errorMessage":{}"#,
            serde_json::Value::from(message)
        ),
    );
    parse_result(&output(0, &text, ""), &id(FRESH_ID))
}

#[test]
fn a_model_host_that_cannot_be_reached_is_unreachable_and_any_other_error_fails() {
    for message in [
        r#"502 "unreachable""#,
        "503 Service Unavailable",
        "fetch failed: connect ECONNREFUSED 192.0.2.9:11434",
        "read ECONNRESET",
    ] {
        assert_eq!(
            errored_with(message),
            Err(AgentError::Unreachable(Harness::Pi, message.into())),
            "{message}"
        );
    }
    // A refused connection is also what a pi that exits non-zero prints.
    let err = parse_result(
        &output(1, "", "Error: connect ECONNREFUSED 192.0.2.9:11434"),
        &id(FRESH_ID),
    )
    .unwrap_err();
    assert!(
        matches!(err, AgentError::Unreachable(Harness::Pi, _)),
        "{err:?}"
    );

    let message = r#"400 "context length exceeded""#;
    assert_eq!(
        errored_with(message),
        Err(AgentError::Failed(Harness::Pi, message.into()))
    );
    // A status number inside other text is not the host's status.
    let message = "the model said 502 things";
    assert_eq!(
        errored_with(message),
        Err(AgentError::Failed(Harness::Pi, message.into()))
    );
}
