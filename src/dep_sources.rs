//! The installed sources and docs of dependencies and toolchains, which every
//! call reads and none of them writes
//!
//! A reviewer that checks a call against the crate it calls reads that crate's
//! source, and a worker reads the version its lockfile pins. Each folder comes
//! from the tool's own setting, or the tool's own default where the setting is
//! unset, and a folder that is not on this machine adds nothing. Nothing here
//! is writable: the fence's write list is untouched, and a credential file
//! beside a readable folder stays in [`crate::profile::CREDENTIALS`].

use std::path::{Path, PathBuf};

/// What a cargo home holds of its dependencies' source, at any depth
const CARGO_SRC: [&str; 2] = ["registry/src", "git/checkouts"];

/// What a rustup toolchain holds of the standard library's source and its docs
const RUSTUP_SRC: [&str; 2] = ["lib/rustlib/src", "share/doc"];

/// The folders of a dependency or toolchain a call may read
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct DepSources {
    folders: Vec<PathBuf>,
}

impl DepSources {
    /// Discovered from the toolchain homes under `home` and the variables in `env`.
    /// A tool that is not installed, or names a folder that is not there, adds nothing.
    pub(crate) fn discover(home: &Path, env: &dyn Env) -> Self {
        let mut found = Self::default();
        found.add_cargo(home, env);
        found.add_rustup(home, env);
        found
    }

    // The unpacked crates and the git checkouts, under `CARGO_HOME` and under
    // cargo's own default home.
    fn add_cargo(&mut self, home: &Path, env: &dyn Env) {
        for dir in tool_homes(env, "CARGO_HOME", home, ".cargo") {
            self.add_under(&dir, &CARGO_SRC);
        }
    }

    // Each installed toolchain's standard library source and its docs, under
    // `RUSTUP_HOME` and under rustup's own default home. Toolchains are read in
    // name order, so the folders come out the same way every run.
    fn add_rustup(&mut self, home: &Path, env: &dyn Env) {
        for dir in tool_homes(env, "RUSTUP_HOME", home, ".rustup") {
            let Ok(entries) = std::fs::read_dir(dir.join("toolchains")) else {
                continue;
            };
            let mut toolchains: Vec<PathBuf> = entries
                .filter_map(|entry| entry.ok().map(|e| e.path()))
                .collect();
            toolchains.sort();
            for toolchain in toolchains {
                self.add_under(&toolchain, &RUSTUP_SRC);
            }
        }
    }

    // Each of `subs` under `root` that is a folder
    fn add_under(&mut self, root: &Path, subs: &[&str]) {
        self.folders.extend(
            subs.iter()
                .map(|sub| root.join(sub))
                .filter(|folder| folder.is_dir()),
        );
    }

    /// The folders, in the order they were found
    pub(crate) fn folders(&self) -> &[PathBuf] {
        &self.folders
    }
}

/// One environment variable, or `None` when it is unset
pub(crate) trait Env {
    fn get(&self, name: &str) -> Option<String>;
}

/// The process's own environment, which is what each tool the call runs reads
pub(crate) struct ProcessEnv;

impl Env for ProcessEnv {
    fn get(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|v| !v.is_empty())
    }
}

/// The folders of the cargo home at `dir` that hold a crate's unpacked source,
/// each one there. A project's `build_env` can put its worker's cargo home in
/// the work item's build folder, whose crates a reviewer reads there.
pub(crate) fn cargo_srcs(dir: &Path) -> Vec<PathBuf> {
    CARGO_SRC
        .iter()
        .map(|sub| dir.join(sub))
        .filter(|folder| folder.is_dir())
        .collect()
}

// The homes a tool keeps its cache in: the one its variable names, then the
// tool's own `default` under `home`. A relative setting is taken from `home`.
// Both are kept, since a machine that moves a cache elsewhere still holds the
// crates the tool's own default folder fetched for every other project.
fn tool_homes(env: &dyn Env, name: &str, home: &Path, default: &str) -> Vec<PathBuf> {
    let default = home.join(default);
    let mut homes = Vec::new();
    if let Some(named) = env.get(name).filter(|v| !v.is_empty()).map(PathBuf::from) {
        let named = match named.is_absolute() {
            true => named,
            false => home.join(named),
        };
        if named != default {
            homes.push(named);
        }
    }
    homes.push(default);
    homes
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    /// The variables in `vars`, as an [`Env`]
    #[derive(Default)]
    struct Fixed(BTreeMap<&'static str, String>);

    impl Env for Fixed {
        fn get(&self, name: &str) -> Option<String> {
            self.0.get(name).cloned()
        }
    }

    // A file at `rel` under `root`, with its folders made
    fn file_at(root: &Path, rel: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "x\n").unwrap();
    }

    #[test]
    fn the_cargo_home_named_by_its_variable_is_the_one_it_reads() {
        let dir = tempfile::tempdir().unwrap();
        let (home, named) = (dir.path(), dir.path().join("builds/7/cargo"));
        file_at(&named, "registry/src/serde-1.0.2/src/lib.rs");
        file_at(&home.join(".cargo"), "registry/src/rand-0.9.0/src/lib.rs");

        let found = DepSources::discover(
            home,
            &Fixed(BTreeMap::from([(
                "CARGO_HOME",
                named.to_string_lossy().into_owned(),
            )])),
        );

        assert_eq!(
            found.folders(),
            [named.join("registry/src"), home.join(".cargo/registry/src"),]
        );
    }

    #[test]
    fn a_rustup_home_gives_each_toolchains_library_source_and_docs() {
        let dir = tempfile::tempdir().unwrap();
        let rustup = dir.path().join(".rustup");
        for rel in [
            "toolchains/nightly-aarch64/lib/rustlib/src/rust/library/std/src/lib.rs",
            "toolchains/stable-aarch64/share/doc/rust/index.html",
            "toolchains/1.88-aarch64/lib/libstd.dylib",
            "toolchains/1.88-aarch64/lib/rustlib/components",
            "downloads/crates",
            "settings.toml",
        ] {
            file_at(&rustup, rel);
        }

        let found = DepSources::discover(dir.path(), &Fixed::default());

        assert_eq!(
            found.folders(),
            [
                rustup.join("toolchains/nightly-aarch64/lib/rustlib/src"),
                rustup.join("toolchains/stable-aarch64/share/doc"),
            ]
        );
    }

    #[test]
    fn a_cargo_home_gives_its_unpacked_sources_and_none_of_its_cache() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path();
        let cargo = home.join(".cargo");
        for rel in [
            "registry/src/index.crates.io-6a1f/shep-core-0.10.0/src/lib.rs",
            "git/checkouts/shep-core-1a2b3c/HEAD/src/lib.rs",
            "registry/cache/index.crates.io-6a1f/shep-core-0.10.0.crate",
            "registry/index/index.crates.io-6a1f/.cache",
            "credentials.toml",
        ] {
            file_at(&cargo, rel);
        }

        let found = DepSources::discover(home, &Fixed(BTreeMap::new()));

        assert_eq!(
            found.folders(),
            [
                home.join(".cargo/registry/src"),
                home.join(".cargo/git/checkouts"),
            ]
        );
    }
}
