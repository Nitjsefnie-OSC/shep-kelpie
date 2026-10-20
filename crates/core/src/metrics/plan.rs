//! An account's plan limits, as the pacer last read them

use super::{Family, Label, Recorder};
use crate::pacer::Reading;
use crate::ports::SeriesKind::Gauge;
use crate::settings::Account;

const USED: Family = Family {
    name: "kelpie_plan_used_ratio",
    kind: Gauge,
    help: "How much of the plan's 5-hour session or weekly window is used, 0 to 1.",
};
const RESETS_AT: Family = Family {
    name: "kelpie_plan_resets_at_seconds",
    kind: Gauge,
    help: "When the plan's session or weekly window resets, in Unix seconds.",
};
const ALLOWANCE: Family = Family {
    name: "kelpie_plan_allowance_ratio",
    kind: Gauge,
    help: "The share of the week today may spend, 0 to 1.",
};
const SPENT_TODAY: Family = Family {
    name: "kelpie_plan_spent_today_ratio",
    kind: Gauge,
    help: "The share of the week spent since the day began, by anyone on the account, 0 to 1.",
};

impl Recorder {
    /// Sets `account`'s plan gauges from the pacer's `reading` of its usage
    pub fn plan(&self, account: Account, reading: &Reading) {
        let account = ("account", account.as_str().to_owned());
        let windows = [("session", &reading.session), ("week", &reading.week)];
        for (name, window) in windows {
            let labels: Vec<Label> = vec![account.clone(), ("window", name.to_owned())];
            self.set(&USED, labels.clone(), f64::from(window.used_pct) / 100.0);
            #[expect(
                clippy::cast_precision_loss,
                reason = "Unix seconds stay far below 2^53"
            )]
            self.set(&RESETS_AT, labels, window.resets_at.0 as f64);
        }
        let allowance = reading.allowance_pct / 100.0;
        self.set(&ALLOWANCE, vec![account.clone()], allowance);
        let spent = f64::from(reading.spent_today_pct) / 100.0;
        self.set(&SPENT_TODAY, vec![account], spent);
    }
}
