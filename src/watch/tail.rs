//! A file read as it grows, a whole line at a time

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

/// A file and how far into it has been read
#[derive(Debug)]
pub(super) struct Tail {
    path: PathBuf,
    offset: u64,
}

impl Tail {
    /// A tail that reads `path` from its first line
    pub fn from_start(path: PathBuf) -> Self {
        Self { path, offset: 0 }
    }

    /// A tail that reads only what `path` gets from now on
    pub fn at_end(path: PathBuf) -> Self {
        let offset = std::fs::metadata(&path).map_or(0, |m| m.len());
        Self { path, offset }
    }

    /// The whole lines written since the last call
    ///
    /// A line still being written waits for its end. A file that shrank
    /// was replaced, and is read from its start. A bad byte costs only its
    /// own line.
    pub fn lines(&mut self) -> Vec<String> {
        let Ok(mut file) = std::fs::File::open(&self.path) else {
            return Vec::new();
        };
        let Ok(len) = file.metadata().map(|m| m.len()) else {
            return Vec::new();
        };
        if len < self.offset {
            self.offset = 0;
        }
        if len == self.offset || file.seek(SeekFrom::Start(self.offset)).is_err() {
            return Vec::new();
        }
        let mut bytes = Vec::new();
        if file
            .take(len - self.offset)
            .read_to_end(&mut bytes)
            .is_err()
        {
            return Vec::new();
        }
        let Some(end) = bytes.iter().rposition(|&b| b == b'\n') else {
            return Vec::new();
        };
        self.offset += end as u64 + 1;
        String::from_utf8_lossy(&bytes[..end])
            .split('\n')
            .map(|line| line.trim_end_matches('\r').to_owned())
            .filter(|line| !line.is_empty())
            .collect()
    }
}
