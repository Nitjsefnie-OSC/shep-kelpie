//! The folder a rig works in

use std::path::{Path, PathBuf};

use tempfile::TempDir;

use super::*;

/// A rig's home: one it made and removes, or one an earlier rig made
#[derive(Debug)]
pub(crate) enum Home {
    Made(TempDir),
    Kept(PathBuf),
}

impl Home {
    pub(super) fn made() -> Self {
        Self::Made(tempfile::tempdir().unwrap())
    }

    pub(crate) fn path(&self) -> &Path {
        match self {
            Self::Made(dir) => dir.path(),
            Self::Kept(path) => path,
        }
    }
}

impl Rig {
    /// The stand-ins over `home`, which holds whatever it holds
    pub(super) fn stand_ins(project: &str, home: Home) -> Self {
        let meter = FakeMeter::idle();
        let clock = FakeClock::at(Self::EPOCH);
        let rig = Self {
            project: ProjectName::try_from(project).unwrap(),
            claude: FakeClaude::metered(meter.clone()),
            forge: FakeForge::new(home.path().join("origin.git")),
            git: FakeGit::default(),
            meter,
            codex_meter: FakeMeter::idle(),
            reviewer: FakeReviewer::default(),
            local_leases: LocalReviewer::default(),
            alerts: FakeAlerts::on(clock.clone()),
            leases: FakeLeases::default(),
            tests: FakeTests::default(),
            github: FakeGithub::new(clock.clone(), "shep-pm"),
            pace: Arc::new(LocalPace::with(CallPacer::new(usize::MAX, usize::MAX))),
            clock,
            home,
        };
        rig.forge.threads.use_clock(rig.clock.clone());
        rig.github.use_threads(rig.forge.coderabbit.clone());
        rig
    }

    /// A rig over the home an earlier rig made, as another image of the
    /// runner sees it after a handover: its files are there and the stand-ins
    /// are new
    pub(crate) fn over(project: &str, home: &Path) -> Self {
        Self::stand_ins(project, Home::Kept(home.to_owned()))
    }
}
