//! The installed sources and docs of dependencies and toolchains, which every
//! call the runner makes reads and none of them writes
//!
//! They go in the call's read list, which each harness turns into its own
//! grant: Claude Code's `additionalDirectories`, and the sandbox's `allowRead`
//! for every harness. They go in the fence's read carve-outs too, so
//! `read_denials` cuts them out of a home the call may not read. The write
//! list is untouched, and a credential beside a readable folder stays denied.
//! They are found as they are granted, not once when the runner starts: a
//! machine gains a toolchain and a project gains a git dependency as it runs.

use std::path::{Path, PathBuf};

use crate::dep_sources::{self, grant, machine_sources};
use crate::ports::{AgentCall, Role, Tools};
use crate::runner::Runner;

impl Runner {
    // Every call that reads code reads the installed sources and docs of its
    // dependencies and toolchains. Two read none: an answer reads only what
    // kelpie put in its prompt, and its harness gives it no read tool; a
    // researcher reads its own folder and the web, and no file of the flock.
    pub(super) fn with_dep_sources(&self, call: &mut AgentCall) {
        if reads_no_file(call) {
            return;
        }
        // Only a call that works a work item has a build folder of its own.
        let build = matches!(call.role, Role::Worker | Role::Reviewer)
            .then_some(call.issue)
            .flatten()
            .map(|ticket| self.paths.build(ticket));
        grant(call, &self.dep_reads(build.as_deref()));
    }

    /// The machine's own sources, for a call that reads the repo and works no
    /// work item of its own: a triager, a contribution's reviewers. They are
    /// built without `prepared`, and a contribution is named by its pull
    /// request, so `paths.build` of it would be another work item's folder.
    pub(in crate::runner) fn with_machine_sources(&self, call: &mut AgentCall) {
        if reads_no_file(call) {
            return;
        }
        grant(call, &machine_sources(&self.home));
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
        // grants when the call starts, and the call's own tool fills the
        // folder, so a grant naming an empty folder may be refused. One that
        // cannot be made is still granted: the tool makes it when it runs.
        let made = |folders: Vec<PathBuf>| {
            for folder in &folders {
                let _ = std::fs::create_dir_all(folder);
            }
            folders
        };
        let mut reads = machine_sources(&self.home);
        if let Some(cargo) = named("CARGO_HOME") {
            reads.extend(made(dep_sources::expected_cargo_srcs(&cargo)));
        }
        if let Some(rustup) = named("RUSTUP_HOME") {
            reads.extend(made(dep_sources::expected_rustup_srcs(&rustup)));
        }
        reads
    }

    // The call's skill and plugin copies sit in kelpie's home, which a fence
    // denies whole: they go in its read carve-outs, so the file tools and the
    // shell both reach the files a skill names under its own folder.
    pub(super) fn with_plugin_reads(&self, call: &mut AgentCall) {
        let Some(fence) = call.reach.fence.as_mut() else {
            return;
        };
        for dir in call.plugin_dirs.clone() {
            if !fence.read.iter().any(|held| held == &dir) {
                fence.read.push(dir);
            }
        }
    }
}

// A call that reads no file of the flock reads none of these either
fn reads_no_file(call: &AgentCall) -> bool {
    matches!(call.tools, Tools::Answer | Tools::Research)
}
