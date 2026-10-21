//! The images a fake forge serves from an issue's text

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::ports::ForgeError;

/// The images behind the links a test serves, and every link asked for
#[derive(Debug, Clone, Default)]
pub struct FakeImages {
    served: Arc<Mutex<BTreeMap<String, Served>>>,
    asked: Arc<Mutex<Vec<String>>>,
}

#[derive(Debug, Clone)]
enum Served {
    Image(&'static str, Vec<u8>),
    Other(&'static str),
}

impl FakeImages {
    /// Serves a PNG of `bytes` at `url`
    pub fn serve_png(&self, url: &str, bytes: &[u8]) {
        let image = Served::Image("png", bytes.to_vec());
        self.served.lock().unwrap().insert(url.to_owned(), image);
    }

    /// Serves something that is no image, of type `content_type`, at `url`
    pub fn serve_other(&self, url: &str, content_type: &'static str) {
        let other = Served::Other(content_type);
        self.served.lock().unwrap().insert(url.to_owned(), other);
    }

    /// Every link a fetch was asked for, in order, whether or not it was served
    pub fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }

    pub(super) fn fetch(&self, url: &str, stem: &Path, most: u64) -> Result<PathBuf, ForgeError> {
        self.asked.lock().unwrap().push(url.to_owned());
        let served = self.served.lock().unwrap().get(url).cloned();
        match served {
            None => Err(ForgeError::Failed(format!("{url}: not found"))),
            Some(Served::Other(kind)) => {
                Err(ForgeError::Failed(format!("it is not an image: {kind}")))
            }
            Some(Served::Image(_, bytes)) if bytes.len() as u64 > most => Err(ForgeError::Failed(
                format!("it is {} bytes, past the {most} allowed", bytes.len()),
            )),
            Some(Served::Image(extension, bytes)) => {
                let mut named = stem.as_os_str().to_owned();
                named.push(".");
                named.push(extension);
                let named = PathBuf::from(named);
                fs::write(&named, bytes)
                    .map_err(|e| ForgeError::Failed(format!("cannot keep it: {e}")))?;
                Ok(named)
            }
        }
    }
}
