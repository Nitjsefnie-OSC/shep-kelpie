//! One adapter for every CLI
//!
//! [`CliAdapter`] implements [`Agents`](crate::ports::Agents) once. It holds
//! what every call does the same way: the check that a session is new or
//! resumed, emptying the scratch folder, the sandbox around the command, the
//! labelled spawn, the mapping of a failed run to an [`AgentError`], and a
//! handover's adoption. For a CLI on a model server it also holds the
//! forwarder and the gateway lease. What is true of one CLI alone is a [`Cli`]
//! in a module of its own, which names no other CLI's module.

mod adapter;
mod served;
mod terminals;

use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

pub use adapter::CliAdapter;

use crate::forwarder::Upstream;
use crate::ports::{
    Adopted, AgentCall, AgentError, AgentReply, CallActivity, Policy, SessionId, Transcript,
};
use crate::settings::Harness;

/// Which of a CLI's two ways of running a session a command is for
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// One call the runner waits for, to its end
    Headless,
    /// The session in the maintainer's terminal
    Terminal,
}

/// The model server a call's CLI dials, through the forwarder
#[derive(Debug, Clone, Copy)]
pub struct Served<'a> {
    /// Where the forwarder passes the call's chat calls, with the gateway
    /// lease it holds named on a worker's
    pub upstream: &'a Upstream,
    /// The socket the sandbox's proxy dials for the model host
    pub socket: &'a Path,
}

/// What a command is built for
#[derive(Debug, Clone, Copy)]
pub struct Launch<'a> {
    /// Whether it is a call or the maintainer's terminal
    pub turn: Turn,
    /// The model server, for a CLI that runs its model on one
    pub served: Option<Served<'a>>,
}

/// How a [`FollowUp`] builds its command
pub type BuildCommand<'a> = Box<dyn FnOnce(&Launch<'_>) -> Result<Command, AgentError> + 'a>;

/// How a [`FollowUp`] completes the reply from what its command printed
pub type CompleteReply<'a> = Box<dyn FnOnce(AgentReply, Result<Output, String>) -> AgentReply + 'a>;

/// A second command run after a call, in the call's sandbox, whose output
/// completes the call's reply
pub struct FollowUp<'a> {
    /// The command, built as the call's is
    pub command: BuildCommand<'a>,
    /// How long it may run
    pub within: Duration,
    /// The reply with what the command printed, or why it printed nothing
    pub read: CompleteReply<'a>,
}

impl fmt::Debug for FollowUp<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FollowUp")
            .field("within", &self.within)
            .finish_non_exhaustive()
    }
}

/// What is true of one CLI, which [`CliAdapter`] runs every call of
///
/// A new CLI is one implementation of this, one row in the harness facts
/// table, and one entry in the routing list. The skeleton of a call is the
/// adapter's: a method here is called where that CLI differs.
pub trait Cli: fmt::Debug + Clone + Send + Sync + 'static {
    /// What a call settles before it spawns and reads its reply with: the
    /// session's record, or nothing
    type Begun: Default + Send;

    /// The harness it is, whose facts the adapter reads
    const HARNESS: Harness;

    /// What this CLI refuses of a call beyond what its facts say, before anything is written
    ///
    /// # Errors
    ///
    /// [`AgentError::Setup`] naming what the call asks that the CLI cannot give.
    fn refuse(&self, call: &AgentCall) -> Result<(), AgentError> {
        let _ = call;
        Ok(())
    }

    /// Writes the files `call` runs from: its settings, the wiring of
    /// `kelpie guard` and `kelpie confine`, and the CLI's own home
    ///
    /// `served` is where the model server's chat calls go, for a CLI on one.
    ///
    /// # Errors
    ///
    /// [`AgentError::Setup`] when a file cannot be written.
    fn write_config(&self, call: &AgentCall, served: Option<&Upstream>) -> Result<(), AgentError>;

    /// A call this CLI answers without running, such as a compaction it only notes
    fn answer(&self, call: &AgentCall) -> Option<Result<AgentReply, AgentError>> {
        let _ = call;
        None
    }

    /// Checks `call`'s session against what the CLI keeps, before the call
    /// waits for a lease or spawns anything
    ///
    /// # Errors
    ///
    /// [`AgentError::NoSession`] for a session to resume that is not there,
    /// and [`AgentError::Setup`] for one that cannot be started or read.
    fn begin(&self, call: &AgentCall, turn: Turn) -> Result<Self::Begun, AgentError> {
        let _ = (call, turn);
        Ok(Self::Begun::default())
    }

    /// Refuses a command that could only fail inside the CLI, before the scratch folder is touched
    ///
    /// # Errors
    ///
    /// [`AgentError::Setup`] with the fix, in place of the CLI's own failure.
    fn ready(&self, call: &AgentCall) -> Result<(), AgentError> {
        let _ = call;
        Ok(())
    }

    /// The call's temporary files, emptied before each command
    fn scratch(&self, call: &AgentCall) -> PathBuf {
        call.settings.with_extension("tmp")
    }

    /// Where the sandbox's own settings for the call go
    fn sandbox_settings(&self, call: &AgentCall) -> PathBuf {
        call.settings.with_extension("sandbox.json")
    }

    /// Folders to make besides the scratch folder, once it is empty
    ///
    /// # Errors
    ///
    /// [`AgentError::Setup`] when one cannot be named.
    fn folders(&self, call: &AgentCall) -> Result<Vec<PathBuf>, AgentError> {
        let _ = call;
        Ok(Vec::new())
    }

    /// The program with its arguments and environment, for the sandbox to wrap
    ///
    /// # Errors
    ///
    /// [`AgentError::Setup`] when an argument cannot be built.
    fn command(
        &self,
        call: &AgentCall,
        begun: &Self::Begun,
        launch: &Launch<'_>,
    ) -> Result<Command, AgentError>;

    /// What the call's command may do in the sandbox. A CLI on a model server
    /// leaves the forwarder to the adapter.
    ///
    /// # Errors
    ///
    /// [`AgentError::Setup`] when a path the policy names cannot be found.
    fn policy(&self, call: &AgentCall, launch: &Launch<'_>) -> Result<Policy, AgentError>;

    /// Reads what the call printed
    ///
    /// # Errors
    ///
    /// [`AgentError`] when the call failed or its output is unreadable.
    fn parse(
        &self,
        call: &AgentCall,
        begun: Self::Begun,
        output: &Output,
    ) -> Result<AgentReply, AgentError>;

    /// What to ask the CLI once the call has ended, for what its output does not say
    fn follow_up<'a>(&'a self, call: &'a AgentCall) -> Option<FollowUp<'a>> {
        let _ = call;
        None
    }

    /// What a call another image started needs from `adopted`, in place of [`Cli::begin`]
    ///
    /// # Errors
    ///
    /// Why the call cannot be finished.
    fn adopted_begin(&self, adopted: &Adopted) -> Result<Self::Begun, String> {
        let _ = adopted;
        Ok(Self::Begun::default())
    }

    /// Reads what an adopted call printed
    ///
    /// # Errors
    ///
    /// As [`Cli::parse`].
    fn parse_adopted(
        &self,
        adopted: &Adopted,
        begun: Self::Begun,
        output: &Output,
    ) -> Result<AgentReply, AgentError>;

    /// The terminal session for `call` is about to start, its command made
    ///
    /// # Errors
    ///
    /// [`AgentError::Setup`] when what the session leaves cannot be noted.
    fn started(&self, call: &AgentCall, begun: &Self::Begun) -> Result<(), AgentError> {
        let _ = (call, begun);
        Ok(())
    }

    /// The terminal session `session` ended, having run with the settings at `settings`
    fn ended(&self, settings: &Path, session: &SessionId) {
        let _ = (settings, session);
    }

    /// Where its session writes the transcript `shep kelpie watch` follows
    fn transcript(&self, call: &AgentCall) -> Option<Transcript> {
        let _ = call;
        None
    }

    /// When the call last showed something
    fn last_active(&self, call: &AgentCall) -> CallActivity {
        let _ = call;
        CallActivity::Untracked
    }
}
