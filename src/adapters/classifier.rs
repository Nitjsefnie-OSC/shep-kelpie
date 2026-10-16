//! The gateway's classifier: one POST to its route, and its answer read
//!
//! The gateway serves the classifier behind a route the project's settings name
//! (`triage.spam.path`), and takes the key its `key_env` holds. The request is
//! one HTTP/1.0 exchange, as every other call to a gateway is: a `state` of the
//! issue, and the one yes/no question `spam`, which the gateway answers with the
//! probability that the answer is yes.
//!
//! ```text
//! POST /classify HTTP/1.0
//! {"state": {"title": "…", "body": "…", "author_association": "contributor",
//!            "account_age_days": 412},
//!  "questions": [{"name": "spam", "text": "Is this issue spam, …?"}]}
//!
//! {"answers": {"spam": 0.92}}
//! ```
//!
//! The answer is read from `answers` under the question's name, or from the
//! name at the top level, which is how a gateway that serves one classifier
//! writes it. A number outside 0 to 1 is no answer at all.

use std::time::Duration;

use serde_json::{Value, json};

use crate::adapters::paddock;
use crate::forwarder::Upstream;
use crate::ports::{Classifier, ClassifierError, Probability, SpamState};

/// How long the classifier gets to connect and to answer. It is asked once per
/// issue, before a call that would take minutes, so it is given no more than a
/// gateway's model list is.
const ANSWER: Duration = Duration::from_secs(10);

/// The question kelpie asks, keyed `spam`
const SPAM_QUESTION: &str =
    "Is this issue spam, an advertisement, or text unrelated to the project?";

/// The gateway's classifier, of either kind
#[derive(Debug, Clone, Copy)]
pub(crate) struct GatewayClassifier;

impl Classifier for GatewayClassifier {
    fn spam(
        &self,
        gateway: &Upstream,
        path: &str,
        issue: &SpamState<'_>,
    ) -> Result<Probability, ClassifierError> {
        if !a_route(path) {
            return Err(ClassifierError::Refused(format!(
                "the classifier's route {path:?} is not one kelpie will send"
            )));
        }
        let body = request(issue);
        let asked = ("POST", path);
        let (status, answer) = paddock::send(gateway, asked, Some(&body), true, ANSWER)
            .map_err(ClassifierError::Unreachable)?;
        match status {
            200 => probability(&answer),
            401 | 403 => Err(ClassifierError::Refused(format!(
                "`POST {path}` took no notice of the key kelpie's settings name"
            ))),
            status => Err(ClassifierError::Refused(format!(
                "`POST {path}` answered HTTP {status}"
            ))),
        }
    }
}

// The request's body: the issue as `state`, and the one question.
fn request(issue: &SpamState<'_>) -> Value {
    json!({
        "state": {
            "title": issue.title,
            "body": issue.body,
            "author_association": issue.author_association,
            "account_age_days": issue.account_age_days,
        },
        "questions": [{ "name": "spam", "text": SPAM_QUESTION }],
    })
}

// The probability the answer gives for `spam`, under `answers` or at the top level.
fn probability(answer: &str) -> Result<Probability, ClassifierError> {
    let unreadable = |what: &str| ClassifierError::Unreadable(what.to_owned());
    let read: Value = serde_json::from_str(answer).map_err(|_| unreadable("it is not JSON"))?;
    let given = read["answers"]["spam"]
        .as_f64()
        .or_else(|| read["spam"].as_f64())
        .ok_or_else(|| unreadable("it names no probability for `spam`"))?;
    Probability::new(given).ok_or_else(|| {
        unreadable(&format!(
            "`spam` is {given}, which is not a probability from 0 to 1"
        ))
    })
}

// Whether `path` is one path, as `triage.spam.path` is checked to be. It goes in
// a request line, so a space or a line break in it would be a header of its own.
fn a_route(path: &str) -> bool {
    path.starts_with('/')
        && !path
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || c == '#')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::AuthorAssociation;
    use crate::settings::{EndpointUrl, GatewayKey};
    use crate::test::{Answer, StandInEndpoint};

    const KEY: &str = "sk-classify";

    fn gateway(server: &StandInEndpoint, key: Option<&str>) -> Upstream {
        let base = EndpointUrl::try_from(server.url().to_owned()).unwrap();
        let upstream = Upstream::new(&base).unwrap();
        match key {
            Some(key) => upstream.with_key(GatewayKey::for_test(key)),
            None => upstream,
        }
    }

    fn issue() -> SpamState<'static> {
        SpamState {
            title: "Buy cheap watches",
            body: "visit our site for cheap watches!!!",
            author_association: Some(AuthorAssociation::None),
            account_age_days: Some(2),
        }
    }

    #[test]
    fn its_request_is_the_issue_as_state_and_the_one_question() {
        let server = StandInEndpoint::start([Answer::Status(200, r#"{"answers":{"spam":0.92}}"#)]);
        let p = GatewayClassifier
            .spam(&gateway(&server, Some(KEY)), "/classify", &issue())
            .unwrap();
        assert_eq!(p, Probability::thousandths(920));
        assert_eq!(server.seen(), ["POST /classify HTTP/1.0"]);
        assert_eq!(server.authorizations(), [Some(format!("Bearer {KEY}"))]);
        let asked = server.requests().remove(0);
        assert_eq!(
            asked,
            json!({
                "state": {
                    "title": "Buy cheap watches",
                    "body": "visit our site for cheap watches!!!",
                    "author_association": "none",
                    "account_age_days": 2,
                },
                "questions": [{ "name": "spam", "text": SPAM_QUESTION }],
            })
        );
    }

    #[test]
    fn an_answer_at_the_top_level_reads_as_well_as_one_under_answers() {
        let server = StandInEndpoint::start([Answer::Status(200, r#"{"spam":0.05}"#)]);
        let p = GatewayClassifier
            .spam(&gateway(&server, Some(KEY)), "/v1/classify", &issue())
            .unwrap();
        assert_eq!(p, Probability::thousandths(50));
    }

    #[test]
    fn a_gateway_that_cannot_be_reached_is_told_as_one() {
        let base = EndpointUrl::try_from(crate::test::unreachable_url()).unwrap();
        let upstream = Upstream::new(&base)
            .unwrap()
            .with_key(GatewayKey::for_test(KEY));
        let err = GatewayClassifier
            .spam(&upstream, "/classify", &issue())
            .unwrap_err();
        assert!(matches!(err, ClassifierError::Unreachable(_)), "{err}");
        assert!(
            err.to_string()
                .starts_with("the classifier could not be reached"),
            "{err}"
        );
    }

    #[test]
    fn a_refused_request_is_refused_without_naming_the_key() {
        for status in [401, 403, 404, 503] {
            let server = StandInEndpoint::start([Answer::Status(status, r#"{"error":"no"}"#)]);
            let err = GatewayClassifier
                .spam(&gateway(&server, Some(KEY)), "/classify", &issue())
                .unwrap_err();
            assert!(
                matches!(err, ClassifierError::Refused(_)),
                "{status}: {err}"
            );
            assert!(!err.to_string().contains(KEY), "{err}");
        }
    }

    #[test]
    fn an_answer_that_is_not_a_probability_is_no_answer() {
        for body in [
            "",
            "not json",
            "{}",
            r#"{"answers": {}}"#,
            r#"{"answers": {"spam": "yes"}}"#,
            r#"{"answers": {"spam": 1.4}}"#,
            r#"{"answers": {"spam": -0.2}}"#,
            r#"{"answers": {"other": 0.9}}"#,
        ] {
            let server = StandInEndpoint::start([Answer::Status(200, body)]);
            let err = GatewayClassifier
                .spam(&gateway(&server, Some(KEY)), "/classify", &issue())
                .unwrap_err();
            assert!(
                matches!(err, ClassifierError::Unreadable(_)),
                "{body}: {err}"
            );
        }
    }

    #[test]
    fn a_route_that_is_not_one_path_is_never_sent() {
        let server = StandInEndpoint::start([Answer::Status(200, r#"{"spam":0.9}"#)]);
        for path in [
            "/classify x",
            "classify",
            "/classify\r\nX-Evil: 1",
            "/classify#x",
        ] {
            let err = GatewayClassifier
                .spam(&gateway(&server, Some(KEY)), path, &issue())
                .unwrap_err();
            assert!(
                matches!(err, ClassifierError::Refused(_)),
                "{path:?}: {err}"
            );
        }
        assert!(server.seen().is_empty(), "nothing was sent");
    }

    #[test]
    fn the_state_holds_nothing_when_the_forge_cannot_say_it() {
        let server = StandInEndpoint::start([Answer::Status(200, r#"{"spam":0}"#)]);
        let unknown = SpamState {
            title: "t",
            body: "b",
            author_association: None,
            account_age_days: None,
        };
        GatewayClassifier
            .spam(&gateway(&server, Some(KEY)), "/classify", &unknown)
            .unwrap();
        let asked = server.requests().remove(0);
        assert_eq!(asked["state"]["author_association"], Value::Null);
        assert_eq!(asked["state"]["account_age_days"], Value::Null);
    }
}
