//! Packages that pi loads as extensions, installed beside the sandbox runtime
//!
//! `tools install` puts each at the version `package.json` pins, in the same
//! `node_modules`. Kelpie loads one with pi's `-e <folder>`, so pi never looks
//! for a package or fetches one itself.

use std::path::PathBuf;

use super::Tools;

/// The package that gives a researcher on pi its web, as `package.json` names it
pub const WEB_ACCESS: &str = "pi-web-access";

impl Tools {
    /// The folder of the pi package `name`, where `tools install` puts it
    ///
    /// The folder is named whether or not it is installed, so a caller asks
    /// [`Tools::has_pi_package`] first.
    pub fn pi_package(&self, name: &str) -> PathBuf {
        self.package(name)
    }

    /// Whether the pi package `name` is installed, with a `package.json` for pi to read
    pub fn has_pi_package(&self, name: &str) -> bool {
        self.pi_package(name).join("package.json").is_file()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_package_is_found_under_node_modules_and_missing_when_not_installed() {
        let dir = tempfile::tempdir().unwrap();
        let tools = Tools::at(dir.path().to_owned());
        assert!(!tools.has_pi_package("pi-web-access"));

        let folder = dir.path().join("node_modules/pi-web-access");
        assert_eq!(tools.pi_package("pi-web-access"), folder);

        // A folder npm left empty is not a package.
        std::fs::create_dir_all(&folder).unwrap();
        assert!(!tools.has_pi_package("pi-web-access"));

        std::fs::write(folder.join("package.json"), "{}").unwrap();
        assert!(tools.has_pi_package("pi-web-access"));
        assert!(!tools.has_pi_package("pi-other"));
    }
}
