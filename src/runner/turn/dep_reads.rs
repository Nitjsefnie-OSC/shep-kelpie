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
    // Every call but an answer reads the installed sources and docs of its
    // dependencies and toolchains. An answer reads only what kelpie put in its
    // prompt, and its harness gives it no read tool.
    pub(super) fn with_dep_sources(&self, call: &mut AgentCall) {
        if call.tools == Tools::Answer {
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

    // The machine's own dependency and toolchain sources, and the crates the
    // build folder `build` holds when the project's `build_env` points cargo's
    // home there.
    fn dep_reads(&self, build: Option<&Path>) -> Vec<PathBuf> {
        let mut reads = self.dep_sources.folders().to_vec();
        let cargo = build.and_then(|build| {
            self.settings
                .worker
                .build_env
                .iter()
                .find(|(name, _)| name.as_str() == "CARGO_HOME")
                .map(|(_, dir)| build.join(dir.as_path()))
        });
        if let Some(cargo) = cargo {
            reads.extend(crate::dep_sources::cargo_srcs(&cargo));
        }
        reads
    }
}
