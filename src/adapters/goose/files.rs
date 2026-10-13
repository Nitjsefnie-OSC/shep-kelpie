//! The files one goose call keeps beside its settings path

use std::path::{Path, PathBuf};

use super::output::Spent;
use crate::ports::{AgentError, SessionId};

/// The files one call keeps beside its settings path
pub(super) struct Files {
    /// goose's own root for the call: its `config`, `data` and `state`
    pub(super) root: PathBuf,
    /// The call's temporary files, emptied before each call
    pub(super) scratch: PathBuf,
    /// Where the sandbox's settings go
    pub(super) sandbox_settings: PathBuf,
}

impl Files {
    /// The call's files, as absolute paths: goose ignores a relative root
    pub(super) fn of(settings: &Path) -> Self {
        let settings = std::path::absolute(settings).unwrap_or_else(|_| settings.to_owned());
        Self {
            root: settings.with_extension("goose"),
            scratch: settings.with_extension("tmp"),
            sandbox_settings: settings.with_extension("sandbox.json"),
        }
    }

    pub(super) fn config(&self) -> PathBuf {
        self.root.join("config")
    }

    pub(super) fn data(&self) -> PathBuf {
        self.root.join("data")
    }

    pub(super) fn state(&self) -> PathBuf {
        self.root.join("state")
    }

    /// Makes the three folders goose keeps its files in
    pub(super) fn prepare(&self) -> Result<(), AgentError> {
        for folder in [self.config(), self.data(), self.state()] {
            std::fs::create_dir_all(&folder).map_err(|e| setup("write", &folder, e))?;
        }
        Ok(())
    }

    /// The tokens goose's session `id` had spent when its last call ended, or
    /// `None` when kelpie never started that session
    pub(super) fn spent(&self, id: &SessionId) -> Result<Option<Spent>, AgentError> {
        let path = self.spent_file(id)?;
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(setup("read", &path, e)),
        };
        serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| AgentError::Setup(format!("cannot read {}: {e}", path.display())))
    }

    /// Notes that session `id` has spent `spent` so far
    pub(super) fn keep(&self, id: &SessionId, spent: Spent) -> Result<(), AgentError> {
        let path = self.spent_file(id)?;
        let folder = self.root.join("spent");
        std::fs::create_dir_all(&folder).map_err(|e| setup("write", &folder, e))?;
        let text = serde_json::to_string(&spent).expect("token counts are JSON");
        std::fs::write(&path, text).map_err(|e| setup("write", &path, e))
    }

    fn spent_file(&self, id: &SessionId) -> Result<PathBuf, AgentError> {
        let plain = !id.0.is_empty()
            && (id.0.chars()).all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !plain {
            return Err(AgentError::Setup(format!(
                "{} is not a session id kelpie makes",
                id.0
            )));
        }
        Ok(self.root.join("spent").join(format!("{}.json", id.0)))
    }
}

fn setup(verb: &str, path: &Path, e: std::io::Error) -> AgentError {
    AgentError::Setup(format!("cannot {verb} {}: {}", path.display(), e.kind()))
}
