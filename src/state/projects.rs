//! The projects under one kelpie home, and their state files
//!
//! A ruling is named by its work item's number, which two projects can
//! share, so a reader that does not know the project looks at every
//! project's state file.

use std::fs;
use std::path::{Path, PathBuf};

use super::{ProjectState, STATE_FILE, StateError, StateStore};

/// The projects' folders under one kelpie home
#[derive(Debug, Clone)]
pub struct Projects {
    folder: PathBuf,
}

impl Projects {
    /// The projects under `kelpie_home`
    pub fn under(kelpie_home: &Path) -> Self {
        Self {
            folder: kelpie_home.to_owned(),
        }
    }

    /// Every project with a state file, by name, and its state as read
    pub fn states(&self) -> Vec<(String, Result<ProjectState, StateError>)> {
        let Ok(entries) = fs::read_dir(&self.folder) else {
            return Vec::new();
        };
        // Kelpie's own files sit beside the projects' folders.
        let mut names: Vec<String> = entries
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|name| !name.starts_with('.') && !crate::home::is_own(name))
            .collect();
        names.sort();
        names
            .into_iter()
            .filter_map(|name| {
                let store = StateStore::new(self.folder.join(&name).join(STATE_FILE));
                let state = store.load().transpose()?;
                Some((name, state))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn states_lists_each_project_and_skips_kelpie_s_own_folders() {
        let home = tempfile::tempdir().unwrap();
        for name in ["koji", "rulings", "totp"] {
            let path = home.path().join(name).join(STATE_FILE);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            StateStore::new(path).save(&ProjectState::new()).unwrap();
        }
        fs::create_dir_all(home.path().join("lab")).unwrap();
        let names: Vec<String> = Projects::under(home.path())
            .states()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(names, ["koji"]);
    }
}
