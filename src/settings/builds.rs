//! `[app.dogs.kelpie.builds]`: how much a work item's build folder may hold, and the free space the disk keeps

use schemars::JsonSchema;
use serde::Deserialize;

const MAX_GB: u32 = 20;
const MIN_FREE_GB: u32 = 20;

/// The two limits that keep a build folder, and the disk, from filling
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Builds")]
pub struct Builds {
    /// The most a work item's build folder may hold, in GB. Past it a running
    /// turn is paused, and the cargo cache the ended turn did not touch is
    /// removed before the turn resumes. 0 turns the limit off. 20 when absent.
    #[serde(default = "max_gb")]
    pub max_gb: u32,
    /// The free space the disk holding kelpie's home must keep, in GB. Under
    /// it a running turn is paused, and no turn starts while it stays under.
    /// 0 turns the limit off. 20 when absent.
    #[serde(default = "min_free_gb")]
    pub min_free_gb: u32,
}

fn max_gb() -> u32 {
    MAX_GB
}

fn min_free_gb() -> u32 {
    MIN_FREE_GB
}

impl Default for Builds {
    fn default() -> Self {
        Self {
            max_gb: MAX_GB,
            min_free_gb: MIN_FREE_GB,
        }
    }
}

const GB: u64 = 1_000_000_000;

impl Builds {
    /// The most a build folder may hold, in bytes, and none when the limit is off
    #[inline]
    pub fn max_bytes(self) -> Option<u64> {
        (self.max_gb > 0).then(|| u64::from(self.max_gb) * GB)
    }

    /// The free space the disk must keep, in bytes, and none when the limit is off
    #[inline]
    pub fn min_free_bytes(self) -> Option<u64> {
        (self.min_free_gb > 0).then(|| u64::from(self.min_free_gb) * GB)
    }
}
