//! `kelpie guard --pause=<build>`: the PostToolUse hook that ends a worker's
//! turn at its next tool result while a pause is set
//!
//! A pause sets a flag file in the work item's build folder, the one folder
//! besides its worktree that the worker's sandbox lets the guard read. The
//! hook answers `{"continue": false}` once a tool call returns and the flag
//! is there, which ends the turn before the next model call, and leaves a
//! second file saying so, which is how the runner tells a paused turn from
//! one that ended on its own. pi's guard extension does the same through
//! [`Pause`] and `ctx.abort()`.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::json;

/// How the hook's command line names the build folder that holds the flag,
/// and asks for the PostToolUse hook that ends the turn at a pause
pub const PAUSE_FLAG: &str = "--pause=";

/// What the runner sets while a pause waits for the turn
const REQUESTED: &str = "kelpie-pause";

/// What the hook leaves when it ended the turn
const ENDED: &str = "kelpie-paused";

/// What Claude Code shows of why the turn ended
const REASON: &str = "kelpie is pausing: this turn ends here and resumes at the next start";

/// The build folder `args` names for [`PAUSE_FLAG`], and the rest of them
pub fn pause_flag(args: &[String]) -> (Option<PathBuf>, Vec<String>) {
    let mut build = None;
    let mut rest = Vec::new();
    for arg in args {
        match arg.strip_prefix(PAUSE_FLAG) {
            Some(folder) => build = Some(PathBuf::from(folder)),
            None => rest.push(arg.clone()),
        }
    }
    (build, rest)
}

/// A work item's pause: the two files in its build folder
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pause {
    requested: PathBuf,
    ended: PathBuf,
}

impl Pause {
    /// The pause of the work item whose build folder is `build`
    pub fn in_build(build: &Path) -> Self {
        Self {
            requested: build.join(REQUESTED),
            ended: build.join(ENDED),
        }
    }

    /// The file a pause sets
    pub fn requested_file(&self) -> &Path {
        &self.requested
    }

    /// The file the turn leaves when it ends for the pause
    pub fn ended_file(&self) -> &Path {
        &self.ended
    }

    /// Sets the pause: the turn in flight ends at its next tool result
    ///
    /// # Errors
    ///
    /// The write's error, when the folder cannot take the file.
    pub fn request(&self) -> io::Result<()> {
        fs::write(&self.requested, "")
    }

    /// Whether a pause is set: the flag file the runner wrote is there
    ///
    /// A folder at that path is no pause. It is what a failed [`Pause::request`]
    /// leaves behind, and a turn is never ended for a pause no one set.
    pub fn is_requested(&self) -> bool {
        self.requested.is_file()
    }

    /// Records that the turn ended for the pause, and answers with what
    /// Claude Code's hook prints to end it
    ///
    /// None when the marker cannot be written: the runner could not tell the
    /// turn was paused, so it goes on to its next tool call and tries again.
    pub fn end_turn(&self) -> Option<String> {
        fs::write(&self.ended, "").ok()?;
        Some(json!({ "continue": false, "stopReason": REASON }).to_string())
    }

    /// Whether the turn ended for the pause
    pub fn ended_turn(&self) -> bool {
        self.ended.exists()
    }

    /// Takes the pause away, leaving a turn that already ended for it so
    pub fn withdraw(&self) {
        // Gone already is what it asks for.
        let _ = fs::remove_file(&self.requested);
    }

    /// Takes both files away, for a turn that begins or ends
    pub fn clear(&self) {
        for file in [&self.requested, &self.ended] {
            // Gone already is what it asks for.
            let _ = fs::remove_file(file);
        }
    }
}

/// What the PostToolUse hook prints for a tool call that has returned: the
/// answer that ends the turn while a pause is set, else nothing
pub fn judge_pause(build: &Path) -> Option<String> {
    let pause = Pause::in_build(build);
    pause.is_requested().then(|| pause.end_turn()).flatten()
}
