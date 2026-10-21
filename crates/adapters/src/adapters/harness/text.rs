//! Small helpers every adapter reads its output and files with

use std::path::{Path, PathBuf};

use crate::ports::KeptServer;
use crate::settings::{EndpointUrl, GatewayName, ModelHost};

/// How much of a CLI's output an error carries: its end, where the reason is
pub(crate) const ERROR_TAIL: usize = 2048;

/// The last `ERROR_TAIL` bytes of `text`, since a worker's output runs to
/// megabytes and an error reaches a ruling, the state file and the webhook.
pub(crate) fn tail(text: &str) -> &str {
    let mut start = text.len().saturating_sub(ERROR_TAIL);
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// Where the prompt that a compaction hands back is kept, beside the call's
/// `settings` file
pub(crate) fn prompt_file(settings: &Path) -> PathBuf {
    settings.with_extension("prompt.md")
}

/// How a call's model server is written in a [`Kept`](crate::ports::Kept)
pub(crate) fn kept_server(host: &ModelHost) -> KeptServer {
    match host {
        ModelHost::Url(url) => KeptServer::Url(url.as_str().to_owned()),
        ModelHost::Gateway(name) => KeptServer::Gateway(name.to_string()),
    }
}

/// The model host a [`KeptServer`] names
///
/// # Errors
///
/// A message when the carried URL or gateway name is no longer valid.
pub(crate) fn host_of(server: &KeptServer) -> Result<ModelHost, String> {
    let unusable =
        |what: &str, e: &str| format!("the carried model server {what} is unusable: {e}");
    match server {
        KeptServer::Url(url) => EndpointUrl::try_from(url.clone())
            .map(ModelHost::Url)
            .map_err(|e| unusable("URL", e)),
        KeptServer::Gateway(name) => GatewayName::try_from(name.clone())
            .map(ModelHost::Gateway)
            .map_err(|e| unusable("gateway", e)),
    }
}
