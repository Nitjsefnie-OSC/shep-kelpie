//! What a CLI keeps of one session between its calls, one JSON file per session id

use std::marker::PhantomData;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::ports::{AgentError, SessionId};

/// The records of one kind in a folder, by the session id kelpie gave the session
///
/// A CLI that names its own sessions keeps which of them each id is here, and
/// one that counts tokens across calls keeps its totals here.
#[derive(Debug)]
pub(crate) struct Records<T> {
    folder: PathBuf,
    kind: PhantomData<fn() -> T>,
}

impl<T> Clone for Records<T> {
    fn clone(&self) -> Self {
        Self {
            folder: self.folder.clone(),
            kind: PhantomData,
        }
    }
}

impl<T: Serialize + DeserializeOwned> Records<T> {
    /// The records in `folder`, which `put` makes
    pub(crate) fn in_folder(folder: PathBuf) -> Self {
        Self {
            folder,
            kind: PhantomData,
        }
    }

    /// The record of session `id`, or `None` when kelpie never kept one
    ///
    /// # Errors
    ///
    /// [`AgentError::Setup`] when `id` is not one kelpie makes, or the file cannot be read.
    pub(crate) fn get(&self, id: &SessionId) -> Result<Option<T>, AgentError> {
        let path = self.file(id)?;
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(failed("read", &path, e.kind())),
        };
        serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| AgentError::Setup(format!("cannot read {}: {e}", path.display())))
    }

    /// Keeps `record` as the record of session `id`
    ///
    /// # Errors
    ///
    /// [`AgentError::Setup`] when `id` is not one kelpie makes, or the file cannot be written.
    pub(crate) fn put(&self, id: &SessionId, record: &T) -> Result<(), AgentError> {
        let path = self.file(id)?;
        std::fs::create_dir_all(&self.folder)
            .map_err(|e| failed("write", &self.folder, e.kind()))?;
        let text = serde_json::to_string(record).expect("a record is JSON");
        std::fs::write(&path, text).map_err(|e| failed("write", &path, e.kind()))
    }

    /// The file that holds the record of session `id`
    ///
    /// # Errors
    ///
    /// [`AgentError::Setup`] when `id` is not one kelpie makes.
    pub(crate) fn file(&self, id: &SessionId) -> Result<PathBuf, AgentError> {
        let plain = !id.0.is_empty()
            && (id.0.chars()).all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !plain {
            return Err(AgentError::Setup(format!(
                "{} is not a session id kelpie makes",
                id.0
            )));
        }
        Ok(self.folder.join(format!("{}.json", id.0)))
    }
}

fn failed(verb: &str, path: &Path, kind: std::io::ErrorKind) -> AgentError {
    AgentError::Setup(format!("cannot {verb} {}: {kind}", path.display()))
}
