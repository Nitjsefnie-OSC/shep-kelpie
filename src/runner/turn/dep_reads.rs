//! The installed sources and docs of dependencies and toolchains, which every
//! call the runner makes reads and none of them writes
//!
//! They go in the call's read list, which each harness turns into its own
//! grant: Claude Code's `additionalDirectories`, and the sandbox's `allowRead`
//! for every harness. They go in the fence's read carve-outs too, so
//! `read_denials` cuts them out of a home the call may not read. The write
//! list is untouched, and a credential beside a readable folder stays denied.

use std::path::{Path, PathBuf};

use crate::ports::{AgentCall, Role, Tools};
use crate::runner::Runner;

impl Runner {
    // Every call that reads code reads the installed sources and docs of its
    // dependencies and toolchains. Two read none: an answer reads only what
    // kelpie put in its prompt, and its harness gives it no read tool; a
    // researcher reads its own folder and the web, and no file of the flock.
    pub(super) fn with_dep_sources(&self, call: &mut AgentCall) {
        if matches!(call.tools, Tools::Answer | Tools::Research) {
            return;
        }
        // Only a call that works a work item has a build folder of its own.
        let build = matches!(call.role, Role::Worker | Role::Reviewer | Role::Triager)
            .then(|| self.paths.build(call.issue));
        let held = |folders: &[PathBuf], folder: &Path| {
            folders
                .iter()
                .any(|h| folder == h.as_path() || folder.starts_with(h))
        };
        for folder in self.dep_reads(build.as_deref()) {
            if held(&call.reach.read, &folder) {
                continue;
            }
            call.reach.read.push(folder.clone());
            if let Some(fence) = call
                .reach
                .fence
                .as_mut()
                .filter(|fence| !held(&fence.read, &folder))
            {
                fence.read.push(folder);
            }
        }
    }

    // The machine's own dependency and toolchain sources, and the folders the
    // build folder `build` is expected to hold when the project's `build_env`
    // points a tool's home there.
    fn dep_reads(&self, build: Option<&Path>) -> Vec<PathBuf> {
        let named = |name: &str| {
            build.and_then(|build| {
                self.settings
                    .worker
                    .build_env
                    .iter()
                    .find(|(key, _)| key.as_str() == name)
                    .map(|(_, dir)| build.join(dir.as_path()))
            })
        };
        // A build-local folder is made as it is granted. A harness judges its
        // grants when the call starts, and the call's own tool is what fills the
        // folder, so a grant left naming an empty folder may be refused, and the
        // source stays unreadable until the next call. One that cannot be made is
        // still granted: the tool makes it when it runs, and the call is no worse
        // off than it was.
        let made = |folders: Vec<PathBuf>| {
            for folder in &folders {
                let _ = std::fs::create_dir_all(folder);
            }
            folders
        };
        let mut reads = self.dep_sources.folders().to_vec();
        if let Some(cargo) = named("CARGO_HOME") {
            reads.extend(made(crate::dep_sources::expected_cargo_srcs(&cargo)));
        }
        if let Some(rustup) = named("RUSTUP_HOME") {
            reads.extend(made(crate::dep_sources::expected_rustup_srcs(&rustup)));
        }
        reads
    }
}
