//! The files one goose call keeps beside its settings path

use std::path::{Path, PathBuf};

use super::output::Spent;
use crate::adapters::harness::Records;
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

    /// The files of the call whose record is `record`, which sits beside its
    /// settings as `<stem>.call.json` for the settings `<stem>.json`
    pub(super) fn beside(record: &Path) -> Self {
        let name = record.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let stem = name.strip_suffix(".call.json").unwrap_or(name);
        Self::of(&record.with_file_name(format!("{stem}.json")))
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

    /// The folder goose reads user plugins from under the root
    pub(super) fn plugins(&self) -> PathBuf {
        self.root.join(".agents").join("plugins")
    }

    /// goose's config file, which names the plugins that are on
    pub(super) fn config_file(&self) -> PathBuf {
        self.config().join("config.yaml")
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
        self.totals().get(id)
    }

    /// Notes that session `id` has spent `spent` so far
    pub(super) fn keep(&self, id: &SessionId, spent: Spent) -> Result<(), AgentError> {
        self.totals().put(id, &spent)
    }

    /// The keys of the MCP servers session `id` was created with, or `None` when kelpie never
    /// recorded any for it
    pub(super) fn servers(&self, id: &SessionId) -> Result<Option<Vec<String>>, AgentError> {
        self.created_with().get(id)
    }

    /// Notes that session `id` was created with the servers `keys`
    pub(super) fn keep_servers(&self, id: &SessionId, keys: &[String]) -> Result<(), AgentError> {
        self.created_with().put(id, &keys.to_vec())
    }

    fn totals(&self) -> Records<Spent> {
        Records::in_folder(self.root.join("spent"))
    }

    fn created_with(&self) -> Records<Vec<String>> {
        Records::in_folder(self.root.join("servers"))
    }
}

fn setup(verb: &str, path: &Path, e: std::io::Error) -> AgentError {
    AgentError::Setup(format!("cannot {verb} {}: {}", path.display(), e.kind()))
}
