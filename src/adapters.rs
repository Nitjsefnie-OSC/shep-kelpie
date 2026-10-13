//! The real ports: the system clock, `gh`, `curl`, the GitHub App's calls,
//! the `claude` command line, its `/usage`, the local review
//! round, the machine's sandbox programs, and the dog's
//! leases over the shepherd channel

use std::time::{SystemTime, UNIX_EPOCH};

use crate::ports::{Clock, Timestamp};

mod claude;
mod codex;
mod codex_usage;
mod curl;
mod door_pace;
mod forwarder;
pub(crate) mod gh;
mod git;
mod github;
mod gpu;
mod host;
mod leases;
mod local;
mod ntfy;
pub(crate) mod paddock;
mod pi;
mod process;
mod srt;
mod stop;
mod test_runner;
mod usage;

#[cfg(test)]
pub(crate) use claude::sandbox::fence_policy;
pub(crate) use claude::settings::REVIEW_TOOLS as CLAUDE_REVIEW_TOOLS;
pub(crate) use claude::settings::settings as claude_settings;
pub(crate) use claude::settings::shell_quote;
pub(crate) use claude::settings::write_only as claude_write_only;
#[cfg(test)]
pub(crate) use claude::write_settings as write_claude_settings;
pub use claude::{ClaudeCli, LOGIN_FIX, LOGIN_VAR, LambLabels, LoginToken};
pub use codex::CodexCli;
pub use codex_usage::CodexMeter;
pub use curl::Curl;
pub use door_pace::DoorPace;
pub use gh::{Gh, GhApps};
pub use git::SystemGit;
pub use github::{CurlGithub, Openssl};
pub use gpu::GpuCurl;
pub use host::SystemHost;
pub use leases::ShepLeases;
pub use local::LocalReviewer;
pub use pi::PiCli;
pub use srt::SandboxRuntime;
#[cfg(test)]
pub(crate) use srt::srt_settings;
pub use stop::stop_calls;
pub use test_runner::SandboxedTests;
pub use usage::UsageMeter;

/// The machine's wall clock
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let since_epoch = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        Timestamp(since_epoch.as_secs())
    }
}
