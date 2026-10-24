//! A git fetch the test holds open

use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::*;

/// A fetch from `origin` that waits here until this is dropped
///
/// The project's checkout runs `remote.origin.uploadpack` as a script in place of git's own, which
/// holds the next fetch open. Every fetch made after it goes through.
#[derive(Debug)]
pub struct HeldGit {
    folder: PathBuf,
}

impl HeldGit {
    /// Whether a fetch reached origin and waits here, within `within`
    pub fn waiting(&self, within: Duration) -> bool {
        let until = Instant::now() + within;
        while !self.folder.join("entered").exists() {
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        true
    }
}

impl Drop for HeldGit {
    fn drop(&mut self) {
        let _ = std::fs::write(self.folder.join("release"), "");
    }
}

impl Rig {
    /// Holds the next fetch from origin open, and lets every other through, so a test can ask a
    /// trigger of a runner whose pass is in the middle of a git read
    pub fn hold_next_git_fetch(&self) -> HeldGit {
        self.hold_git_fetch_after(0)
    }

    /// Holds the fetch from origin after the next `skipped`, which go through
    ///
    /// A step fetches in more than one place, so a test names the read it holds by how many
    /// fetches the step makes first.
    pub fn hold_git_fetch_after(&self, skipped: usize) -> HeldGit {
        let folder = self.home.path().join("held-git");
        std::fs::create_dir_all(&folder).unwrap();
        let script = folder.join("upload-pack.sh");
        let (armed, entered, release, skips) = (
            path(&folder.join("armed")).to_owned(),
            path(&folder.join("entered")).to_owned(),
            path(&folder.join("release")).to_owned(),
            path(&folder.join("skips")).to_owned(),
        );
        let body = format!(
            "#!/bin/sh\n\
             skips=$(cat '{skips}')\n\
             if [ \"$skips\" -gt 0 ]; then\n\
             echo $((skips - 1)) > '{skips}'\n\
             elif mv '{armed}' '{entered}' 2>/dev/null; then\n\
             while [ ! -e '{release}' ]; do sleep 0.01; done\n\
             fi\n\
             exec git upload-pack \"$@\"\n"
        );
        let _ = std::fs::remove_file(&script);
        write_script(&script, &body);
        std::fs::write(folder.join("armed"), "").unwrap();
        std::fs::write(folder.join("skips"), skipped.to_string()).unwrap();
        git(
            &self.repo(),
            &["config", "remote.origin.uploadpack", path(&script)],
        );
        HeldGit { folder }
    }
}
