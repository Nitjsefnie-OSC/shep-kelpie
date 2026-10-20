//! The real ports: `gh`, `curl`, the GitHub App's calls,
//! the `claude` command line, its `/usage`, the local review
//! round, the machine's sandbox programs, and the dog's
//! leases over the shepherd channel

pub(crate) mod bridge;
mod classifier;
mod claude;
mod codex;
mod codex_usage;
mod curl;
mod dev_server;
mod disk;
mod door_pace;
mod e2e_runner;
pub(crate) mod forwarder;
pub(crate) mod gh;
mod git;
mod github;
pub(crate) mod goose;
mod gpu;
pub mod host;
mod leases;
mod local;
pub(crate) mod mcp;
mod metric_sink;
mod ntfy;
pub(crate) mod openai_gateway;
pub(crate) mod paddock;
mod pages;
mod pi;
mod process;
mod skills;
mod srt;
mod stop;
mod test_runner;
mod unreachable;
mod usage;

pub(crate) use classifier::GatewayClassifier;
#[cfg(test)]
pub(crate) use claude::sandbox::fence_policy;
pub(crate) use claude::settings::settings as claude_settings;
pub(crate) use claude::settings::write_only as claude_write_only;
#[cfg(test)]
pub(crate) use claude::write_settings as write_claude_settings;
pub use claude::{ClaudeCli, LOGIN_FIX, LOGIN_VAR, LambLabels, LoginToken};
pub use codex::CodexCli;
pub(crate) use codex::mcp::refusal as codex_mcp_refusal;
pub use codex_usage::CodexMeter;
pub use curl::Curl;
pub use dev_server::SandboxedServers;
pub use disk::StatDisk;
pub use door_pace::DoorPace;
pub use e2e_runner::DogE2e;
pub use gh::{Etags, Gh, GhApps};
pub use git::SystemGit;
pub use github::{CurlGithub, Openssl};
pub(crate) use goose::mcp::{refusal as goose_mcp_refusal, skipped_env as goose_mcp_skipped_env};
pub use goose::{GOOSE_MINIMUM, GooseCli};
pub use gpu::GpuCurl;
pub use host::SystemHost;
pub use leases::{Notes, ShepLeases};
pub use local::LocalReviewer;
pub use pages::PlaywrightPages;
pub(crate) use pi::mcp::refusal as pi_mcp_refusal;
pub use pi::{PI_MINIMUM, PiCli};
pub use process::{Sealed, end_groups, feeding};
pub use srt::SandboxRuntime;
#[cfg(test)]
pub(crate) use srt::srt_settings;
pub use stop::stop_calls;
pub use test_runner::SandboxedTests;
pub use usage::UsageMeter;
