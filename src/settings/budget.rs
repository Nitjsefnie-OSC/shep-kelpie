//! What one work item may spend, `[app.dogs.kelpie.budget]`

use schemars::JsonSchema;
use serde::Deserialize;

/// What one work item may spend before kelpie raises a ruling
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "Budget")]
pub struct Budget {
    /// The most a work item's priced calls may cost, in US dollars, before
    /// kelpie raises a `stuck` ruling for `over-budget` instead of starting
    /// the worker's next turn
    ///
    /// The worker's and the reviewers' calls count, and a call whose harness
    /// reports no dollars counts nothing. A yes doubles the work item's cap,
    /// and so does a no with a note. More than 0, and rounded to a cent.
    /// 10 when absent.
    #[serde(default)]
    pub per_item_usd: PerItemUsd,
}

/// A cap on one work item's spend, kept in whole cents so the state can
/// compare it
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(try_from = "f64")]
// schemars describes a `try_from` type by its source, so the bounds go here.
#[schemars(extend("exclusiveMinimum" = 0, "default" = 10))]
pub struct PerItemUsd(u64);

impl PerItemUsd {
    /// The cap in whole cents
    #[inline]
    pub fn cents(self) -> u64 {
        self.0
    }
}

impl Default for PerItemUsd {
    fn default() -> Self {
        Self(1000)
    }
}

impl TryFrom<f64> for PerItemUsd {
    type Error = &'static str;

    fn try_from(usd: f64) -> Result<Self, Self::Error> {
        let cents = (usd * 100.0).round();
        // A cap below a cent would rule on the first call and double to
        // nothing, and the cast saturates, so an absurd figure cannot wrap.
        match usd.is_finite() && cents >= 1.0 {
            true => Ok(Self(cents as u64)),
            false => Err("must be a number of dollars above 0, such as 10 or 2.5"),
        }
    }
}
