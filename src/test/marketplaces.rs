//! The rig's marketplace git: every fetch recorded, writing the marketplace a
//! test scripted for the URL, and failing while a test says so

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::ports::Marketplaces;

// A marketplace's files: each a path under its folder, and its text
type Files = Vec<(String, String)>;

/// A git that writes scripted marketplaces instead of cloning them
#[derive(Debug, Clone, Default)]
pub(crate) struct FakeMarketplaces {
    fetched: Arc<Mutex<Vec<(String, PathBuf)>>>,
    // The marketplace's files for a URL: path under the folder, and its text
    held: Arc<Mutex<BTreeMap<String, Files>>>,
    down: Arc<AtomicBool>,
}

impl FakeMarketplaces {
    /// Makes a fetch of `url` write `files`, each a path under the folder and its text
    pub(crate) fn hold(&self, url: &str, files: &[(&str, &str)]) {
        let files = files
            .iter()
            .map(|(path, text)| ((*path).to_owned(), (*text).to_owned()))
            .collect();
        self.held.lock().unwrap().insert(url.to_owned(), files);
    }

    /// Every fetch tried, failed ones included: the URL and the folder asked for
    pub(crate) fn fetched(&self) -> Vec<(String, PathBuf)> {
        self.fetched.lock().unwrap().clone()
    }

    /// Makes fetches fail as git with no network would, or work again
    pub(crate) fn set_down(&self, down: bool) {
        self.down.store(down, Ordering::SeqCst);
    }
}

impl Marketplaces for FakeMarketplaces {
    fn fetch(&self, url: &str, into: &Path) -> Result<(), String> {
        self.fetched
            .lock()
            .unwrap()
            .push((url.to_owned(), into.to_owned()));
        if self.down.load(Ordering::SeqCst) {
            return Err(format!("could not resolve host for {url}"));
        }
        let held = self.held.lock().unwrap();
        let files = held.get(url).ok_or_else(|| format!("{url} is not there"))?;
        for (path, text) in files {
            let file = into.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, text).unwrap();
        }
        Ok(())
    }
}
