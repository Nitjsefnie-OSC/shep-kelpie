//! A reviewer that runs on its own: a command, or kelpie's reviewer over an
//! OpenAI-compatible server
//!
//! A reviewer agent file on the `command` or `endpoint` harness defines one.

use std::path::PathBuf;

use schemars::JsonSchema;
use serde::Deserialize;

use super::NonBlank;
use super::gateway::ModelHost;

// The smallest context the endpoint reviewer accepts, in tokens. At 4096 a
// reply keeps 1,024 and the prompt about 500, leaving about 2,600 for a diff.
const MIN_CONTEXT: u32 = 4096;

/// What runs a local round
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LocalRound {
    /// Kelpie's own reviewer, over an OpenAI-compatible server
    Endpoint(Endpoint),
    /// A command that keeps the README's contract
    Command(LocalCommand),
}

/// An OpenAI-compatible server and the model kelpie's reviewer asks
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    /// The server's base URL, or the gateway in front of it
    pub host: ModelHost,
    /// The model's name as the server knows it
    pub model: NonBlank,
    /// The context size the server gives the model, in tokens
    pub context: ContextSize,
    /// The agent file's body. It replaces the opening sentence and what the
    /// fixed prompt asks to report; kelpie still adds the diff's format and
    /// the answer's format. None when the file has no body.
    pub prompt: Option<String>,
}

impl Endpoint {
    /// The `max_tokens` a round asks the model for: a quarter of its context
    /// is kept for the reply, which a thinking model fills
    pub fn reply_tokens(&self) -> u32 {
        self.context.get() / 4
    }
}

/// A command run for each local round
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalCommand {
    /// Its path. A leading `~/` is the home folder.
    pub command: PathBuf,
}

/// An `http://` or `https://` URL, kept without a trailing `/`
#[derive(Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(try_from = "String")]
// schemars describes a `try_from` type by its source, so the bound goes here.
#[schemars(extend("pattern" = "^https?://[^\\s/]"))]
pub struct EndpointUrl(String);

impl std::fmt::Debug for EndpointUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EndpointUrl(..)")
    }
}

impl EndpointUrl {
    /// The URL, without a trailing `/`
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for EndpointUrl {
    type Error = &'static str;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        let url = value.trim_end_matches('/');
        let host = url
            .strip_prefix("http://")
            .or_else(|| url.strip_prefix("https://"));
        match host {
            Some(host) if !host.is_empty() && !host.contains(char::is_whitespace) => {
                Ok(Self(url.to_owned()))
            }
            _ => Err("must be an `http://` or `https://` URL"),
        }
    }
}

/// A model's context size in tokens, at least 4096
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(try_from = "i64")]
// schemars describes a `try_from` type by its source, so the bound goes here.
#[schemars(extend("minimum" = MIN_CONTEXT, "maximum" = u32::MAX))]
pub struct ContextSize(u32);

impl ContextSize {
    /// The size in tokens
    #[inline]
    pub fn get(self) -> u32 {
        self.0
    }
}

impl TryFrom<i64> for ContextSize {
    type Error = &'static str;

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match u32::try_from(value) {
            Ok(tokens) if tokens >= MIN_CONTEXT => Ok(Self(tokens)),
            Err(_) if value > 0 => Err("is too large: give the model's context in tokens"),
            _ => Err("must be at least 4096 tokens"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(text: &str) -> EndpointUrl {
        EndpointUrl::try_from(text.to_owned()).unwrap()
    }

    #[test]
    fn an_endpoint_url_does_not_print_itself() {
        assert_eq!(
            format!("{:?}", url("http://gpu-box:9835/metrics")),
            "EndpointUrl(..)"
        );
    }

    #[test]
    fn a_malformed_url_or_context_is_refused() {
        for text in ["localhost:11434", "ftp://x", "http://", "http://a b"] {
            let err = EndpointUrl::try_from(text.to_owned()).unwrap_err();
            assert_eq!(err, "must be an `http://` or `https://` URL", "{text}");
        }
        for context in [0, 4095, -1] {
            let err = ContextSize::try_from(context).unwrap_err();
            assert_eq!(err, "must be at least 4096 tokens", "{context}");
        }
        assert!(
            ContextSize::try_from(5_000_000_000)
                .unwrap_err()
                .contains("too large")
        );
    }
}
