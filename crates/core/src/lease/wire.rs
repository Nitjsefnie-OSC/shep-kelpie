//! How a runner and the dog talk about book leases, through shep
//!
//! A runner sends two running totals per kind as notes on its shepherd
//! channel, `lease.<kind>.want.<epoch>` and `lease.<kind>.return.<epoch>`.
//! Totals survive a dropped note, since the next one carries the count. The
//! epoch in the name ties each total to one run of the runner. The dog
//! grants with a `grant` trigger whose params are `<kind> <epoch>`. What a
//! runner sees of a review window goes up as `window.<kind>.<fact>.<epoch>`.
//! A runner on a build from before notes sent the same names as metric
//! samples, which the dog still reads for one release.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use super::saved::SavedBook;
use super::{Epoch, Holder, LeaseKind};
use crate::home::ProjectName;

/// The trigger the dog grants a lease with
pub const GRANT: &str = "grant";

/// Which of a runner's two totals a note carries
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Total {
    /// How many times this run has asked for the kind
    Want,
    /// How many times this run has given it back
    Return,
}

impl Total {
    fn as_str(self) -> &'static str {
        match self {
            Self::Want => "want",
            Self::Return => "return",
        }
    }
}

/// A lease total's name, read back
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TotalName {
    /// The kind it is about
    pub kind: LeaseKind,
    /// Which total it carries
    pub total: Total,
    /// The run that raised it
    pub epoch: Epoch,
}

impl TotalName {
    /// Reads `lease.<kind>.<want|return>.<epoch>`, or `None` for any other
    /// name
    pub fn parse(name: &str) -> Option<Self> {
        let rest = name.strip_prefix("lease.")?;
        let (rest, epoch) = rest.rsplit_once('.')?;
        let (kind, total) = rest.rsplit_once('.')?;
        let total = match total {
            "want" => Total::Want,
            "return" => Total::Return,
            _ => return None,
        };
        Some(Self {
            kind: LeaseKind::try_from(kind).ok()?,
            total,
            epoch: Epoch(epoch.parse().ok()?),
        })
    }
}

impl fmt::Display for TotalName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { kind, total, epoch } = self;
        write!(f, "lease.{kind}.{}.{}", total.as_str(), epoch.0)
    }
}

/// What a runner saw of a kind's review window, raised as
/// `window.<kind>.<fact>.<epoch>`
///
/// Each carries a Unix time as its value, and a latest value rather than a
/// total, so a repeat changes nothing and a dropped one is covered by the
/// next raise.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WindowFact {
    /// A summon was accepted at this time
    Summoned,
    /// A refusal quoted the window opening at this time
    Opens,
    /// A review footer posted at this time allows this many reviews an hour
    Quota(u32),
}

impl WindowFact {
    fn name(self) -> String {
        match self {
            Self::Summoned => "summoned".into(),
            Self::Opens => "opens".into(),
            Self::Quota(per_hour) => format!("quota-{per_hour}"),
        }
    }

    // A run keeps one latest value of each sort, whatever quota it states.
    fn sort(self) -> u8 {
        match self {
            Self::Summoned => 0,
            Self::Opens => 1,
            Self::Quota(_) => 2,
        }
    }
}

/// A window fact's name, read back
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowName {
    /// The kind whose window it is about
    pub kind: LeaseKind,
    /// What was seen
    pub fact: WindowFact,
    /// The run that raised it
    pub epoch: Epoch,
}

impl WindowName {
    /// Reads `window.<kind>.<fact>.<epoch>`, or `None` for any other name
    pub fn parse(name: &str) -> Option<Self> {
        let rest = name.strip_prefix("window.")?;
        let (rest, epoch) = rest.rsplit_once('.')?;
        let (kind, fact) = rest.rsplit_once('.')?;
        let fact = match fact {
            "summoned" => WindowFact::Summoned,
            "opens" => WindowFact::Opens,
            _ => {
                let per_hour = fact.strip_prefix("quota-")?;
                if !per_hour.bytes().all(|b| b.is_ascii_digit()) {
                    return None;
                }
                WindowFact::Quota(per_hour.parse().ok()?)
            }
        };
        Some(Self {
            kind: LeaseKind::try_from(kind).ok()?,
            fact,
            epoch: Epoch(epoch.parse().ok()?),
        })
    }
}

impl fmt::Display for WindowName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { kind, fact, epoch } = self;
        write!(f, "window.{kind}.{}.{}", fact.name(), epoch.0)
    }
}

/// The params of a `grant` trigger: `<kind> <epoch>`
pub fn grant_params(kind: &LeaseKind, epoch: Epoch) -> String {
    format!("{kind} {}", epoch.0)
}

/// One run's two running totals for one kind
// wire format: changing this is a change to the handover's state format
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Totals {
    /// How many times it has asked
    pub want: u64,
    /// How many times it has given back or withdrawn
    pub give_back: u64,
}

impl Totals {
    /// Whether the run is asking: more wants than give-backs
    pub fn asking(self) -> bool {
        self.want > self.give_back
    }
}

/// What an [`Asker`] hands to the next image of its runner across a handover
///
/// The dog acts only on a total above the last it saw from a run, and a
/// handover keeps the run, so the next image must count on from here.
// wire format: changing this is a change to the handover's state format
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskerCarry {
    totals: Vec<(LeaseKind, Totals)>,
    held: Vec<LeaseKind>,
}

/// A runner's side of its book leases: its totals and what it holds
///
/// A runner waiting on a grant may send [`Asker::notes`] again at any
/// time: the dog reads totals, so repeats change nothing, and they cover
/// a note shep dropped.
#[derive(Debug, Clone)]
pub struct Asker {
    epoch: Epoch,
    totals: BTreeMap<LeaseKind, Totals>,
    held: Vec<LeaseKind>,
    seen: BTreeMap<(LeaseKind, u8), (WindowFact, u64)>,
}

impl Asker {
    /// A fresh run's side, with nothing asked for
    pub fn new(epoch: Epoch) -> Self {
        Self {
            epoch,
            totals: BTreeMap::new(),
            held: Vec::new(),
            seen: BTreeMap::new(),
        }
    }

    /// A run that goes on from what the previous image of the runner kept
    pub fn resume(epoch: Epoch, carried: AskerCarry) -> Self {
        Self {
            epoch,
            totals: carried.totals.into_iter().collect(),
            held: carried.held,
            seen: BTreeMap::new(),
        }
    }

    /// A run that goes on from what the dog's saved `book` says of run
    /// `epoch` of `sheep`, or a fresh one when the book knows no such run
    ///
    /// For a handover from a build that carried no totals. The dog ignores
    /// the totals of a sheep that is no project name, so the book holds no
    /// run of one, and it too starts fresh.
    pub fn from_book(epoch: Epoch, sheep: &str, book: &SavedBook) -> Self {
        let mut asker = Self::new(epoch);
        let Some(run) = book
            .runs
            .iter()
            .find(|run| run.sheep == sheep && run.epoch == epoch)
        else {
            return asker;
        };
        for t in &run.totals {
            let totals = Totals {
                want: t.want,
                give_back: t.give_back,
            };
            asker.totals.insert(t.kind.clone(), totals);
        }
        asker.reconcile(sheep, book);
        asker
    }

    /// Takes as held each kind this run asks for that the dog's saved `book`
    /// says it holds, which a `grant` that arrived while the previous image
    /// handed over would otherwise have left unheard
    ///
    /// The dog saves its book before it delivers a grant, and does not
    /// retry one, so a runner that is asking again repeats a total the dog
    /// ignores. A kind is taken only where the book has heard this run's
    /// latest totals for it: a book that trails may show a hold the dog is
    /// about to end.
    pub fn reconcile(&mut self, sheep: &str, book: &SavedBook) {
        let Ok(project) = ProjectName::try_from(sheep) else {
            return;
        };
        let epoch = self.epoch;
        let Some(run) = book
            .runs
            .iter()
            .find(|run| run.sheep == sheep && run.epoch == epoch)
        else {
            return;
        };
        let heard = |kind: &LeaseKind| {
            let mine = self.totals.get(kind).copied().unwrap_or_default();
            run.totals
                .iter()
                .find(|t| &t.kind == kind)
                .is_some_and(|t| t.want == mine.want && t.give_back == mine.give_back)
        };
        let me = Holder::Runner {
            project,
            epoch: self.epoch,
        };
        for lease in &book.leases {
            let holder = lease.held.as_ref().map(|h| Holder::from(h.holder.clone()));
            let new = !self.held.contains(&lease.kind);
            if holder.as_ref() == Some(&me) && self.asking(&lease.kind) && new && heard(&lease.kind)
            {
                self.held.push(lease.kind.clone());
            }
        }
    }

    /// What the next image of this runner takes up with [`Asker::resume`]
    pub fn keep(&self) -> AskerCarry {
        AskerCarry {
            totals: self.totals.iter().map(|(k, t)| (k.clone(), *t)).collect(),
            held: self.held.clone(),
        }
    }

    /// Tells the dog what this run saw of `kind`'s window: the note to send
    #[must_use = "the dog hears of the window only through this note"]
    pub fn window(&mut self, kind: &LeaseKind, fact: WindowFact, value: u64) -> (String, f64) {
        self.seen.insert((kind.clone(), fact.sort()), (fact, value));
        self.window_note(kind, fact, value)
    }

    fn window_note(&self, kind: &LeaseKind, fact: WindowFact, value: u64) -> (String, f64) {
        let name = WindowName {
            kind: kind.clone(),
            fact,
            epoch: self.epoch,
        };
        // Exact: Unix times and quotas stay far below 2^53.
        #[allow(clippy::cast_precision_loss)]
        (name.to_string(), value as f64)
    }

    /// Asks for `kind`: the note to send, and its value
    #[must_use = "the dog hears of the ask only through this note"]
    pub fn want(&mut self, kind: &LeaseKind) -> (String, f64) {
        let entry = self.totals.entry(kind.clone()).or_default();
        entry.want += 1;
        self.total_note(kind, Total::Want)
    }

    /// Gives `kind` back, or withdraws the ask: the note to send
    #[must_use = "the dog hears of the return only through this note"]
    pub fn give_back(&mut self, kind: &LeaseKind) -> (String, f64) {
        self.held.retain(|k| k != kind);
        let entry = self.totals.entry(kind.clone()).or_default();
        entry.give_back += 1;
        self.total_note(kind, Total::Return)
    }

    /// Takes a `grant` trigger's params, and says which kind is now held
    ///
    /// # Errors
    ///
    /// [`GrantError`] when the params do not parse, name another run, or
    /// grant a kind this run has not asked for.
    pub fn grant(&mut self, params: &str) -> Result<LeaseKind, GrantError> {
        let malformed = || GrantError::Malformed(params.to_owned());
        let (kind, epoch) = params.trim().split_once(' ').ok_or_else(malformed)?;
        let kind = LeaseKind::try_from(kind).map_err(|_| malformed())?;
        let epoch = Epoch(epoch.parse().map_err(|_| malformed())?);
        if epoch != self.epoch {
            return Err(GrantError::OtherRun(epoch));
        }
        if !self.asking(&kind) {
            return Err(GrantError::NotAsked(kind));
        }
        if !self.held.contains(&kind) {
            self.held.push(kind.clone());
        }
        Ok(kind)
    }

    /// Whether this run holds `kind`
    pub fn holds(&self, kind: &LeaseKind) -> bool {
        self.held.contains(kind)
    }

    /// Whether this run has asked for `kind` and not given it back
    pub fn asking(&self, kind: &LeaseKind) -> bool {
        self.totals.get(kind).is_some_and(|t| t.asking())
    }

    /// Every note this run has sent, at its current value
    pub fn notes(&self) -> Vec<(String, f64)> {
        let both = [Total::Want, Total::Return];
        let totals = self
            .totals
            .keys()
            .flat_map(|kind| both.map(|total| self.total_note(kind, total)));
        let seen = self
            .seen
            .iter()
            .map(|((kind, _), (fact, at))| self.window_note(kind, *fact, *at));
        totals.chain(seen).collect()
    }

    fn total_note(&self, kind: &LeaseKind, total: Total) -> (String, f64) {
        let totals = self.totals.get(kind).copied().unwrap_or_default();
        let value = match total {
            Total::Want => totals.want,
            Total::Return => totals.give_back,
        };
        let name = TotalName {
            kind: kind.clone(),
            total,
            epoch: self.epoch,
        };
        // Exact: a run asks far fewer than 2^53 times.
        #[allow(clippy::cast_precision_loss)]
        (name.to_string(), value as f64)
    }
}

/// Why a `grant` was not taken
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantError {
    /// The params are not `<kind> <epoch>`, carrying them
    Malformed(String),
    /// It was meant for another run of this runner
    OtherRun(Epoch),
    /// This run is not asking for that kind
    NotAsked(LeaseKind),
}

impl fmt::Display for GrantError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(params) => write!(f, "a grant takes <kind> <epoch>, not {params:?}"),
            Self::OtherRun(epoch) => write!(f, "that grant was for run {}", epoch.0),
            Self::NotAsked(kind) => write!(f, "this run is not asking for {kind}"),
        }
    }
}

impl core::error::Error for GrantError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test::EPOCH;

    fn stand_in() -> LeaseKind {
        LeaseKind::try_from("stand-in").unwrap()
    }

    #[test]
    fn totals_count_up_under_a_name_that_carries_the_run() {
        let mut asker = Asker::new(Epoch(4242));
        assert_eq!(
            asker.want(&stand_in()),
            ("lease.stand-in.want.4242".into(), 1.0)
        );
        assert_eq!(
            asker.give_back(&stand_in()),
            ("lease.stand-in.return.4242".into(), 1.0)
        );
        assert_eq!(
            asker.want(&stand_in()),
            ("lease.stand-in.want.4242".into(), 2.0)
        );
        assert_eq!(
            asker.notes(),
            [
                ("lease.stand-in.want.4242".into(), 2.0),
                ("lease.stand-in.return.4242".into(), 1.0)
            ]
        );
    }

    #[test]
    fn a_total_s_name_reads_back() {
        assert_eq!(
            TotalName::parse("lease.stand-in.return.7"),
            Some(TotalName {
                kind: stand_in(),
                total: Total::Return,
                epoch: Epoch(7)
            })
        );
        for other in [
            "cpu.load",
            "lease.stand-in.want",
            "lease.stand-in.keep.7",
            "lease.gpu.want.7",
            "lease.Stand.want.7",
            "lease.stand-in.want.-7",
        ] {
            assert_eq!(TotalName::parse(other), None, "{other}");
        }
    }

    #[test]
    fn a_grant_for_this_run_is_held_until_given_back() {
        let mut asker = Asker::new(Epoch(9));
        let _ = asker.want(&stand_in());
        assert_eq!(
            asker.grant(&grant_params(&stand_in(), Epoch(9))),
            Ok(stand_in())
        );
        assert!(asker.holds(&stand_in()));
        let _ = asker.give_back(&stand_in());
        assert!(!asker.holds(&stand_in()));
    }

    #[test]
    fn a_grant_for_another_run_or_an_unasked_kind_is_refused() {
        let mut asker = Asker::new(Epoch(9));
        assert_eq!(
            asker.grant("stand-in 9"),
            Err(GrantError::NotAsked(stand_in()))
        );
        let _ = asker.want(&stand_in());
        assert_eq!(
            asker.grant("stand-in 8"),
            Err(GrantError::OtherRun(Epoch(8)))
        );
        for bad in ["", "stand-in", "stand-in nine", "gpu 9"] {
            assert_eq!(
                asker.grant(bad),
                Err(GrantError::Malformed(bad.into())),
                "{bad:?}"
            );
        }
        assert!(!asker.holds(&stand_in()));
    }

    #[test]
    fn each_kind_keeps_its_own_totals_and_hold() {
        let other = LeaseKind::try_from("other").unwrap();
        let mut asker = Asker::new(Epoch(9));
        let _ = asker.want(&stand_in());
        let _ = asker.want(&other);
        let _ = asker.want(&other);
        asker.grant("other 9").unwrap();
        assert!(asker.holds(&other) && !asker.holds(&stand_in()));
        assert_eq!(
            asker.notes(),
            [
                ("lease.other.want.9".into(), 2.0),
                ("lease.other.return.9".into(), 0.0),
                ("lease.stand-in.want.9".into(), 1.0),
                ("lease.stand-in.return.9".into(), 0.0),
            ]
        );
    }

    #[test]
    fn a_repeated_grant_is_held_once() {
        let mut asker = Asker::new(Epoch(9));
        let _ = asker.want(&stand_in());
        asker.grant("stand-in 9").unwrap();
        asker.grant("stand-in 9").unwrap();
        let _ = asker.give_back(&stand_in());
        assert!(!asker.holds(&stand_in()));
    }

    #[test]
    fn window_facts_go_up_as_latest_values_and_are_raised_again() {
        let coderabbit = LeaseKind::coderabbit();
        let mut asker = Asker::new(Epoch(9));
        assert_eq!(
            asker.window(&coderabbit, WindowFact::Summoned, EPOCH),
            ("window.coderabbit.summoned.9".into(), EPOCH as f64)
        );
        let _ = asker.window(&coderabbit, WindowFact::Quota(1), EPOCH + 100);
        let _ = asker.window(&coderabbit, WindowFact::Quota(10), EPOCH + 200);
        assert_eq!(
            asker.notes(),
            [
                ("window.coderabbit.summoned.9".into(), EPOCH as f64),
                ("window.coderabbit.quota-10.9".into(), (EPOCH + 200) as f64),
            ],
            "one latest quota, carrying its footer's time"
        );
    }

    #[test]
    fn a_window_fact_s_name_reads_back() {
        assert_eq!(
            WindowName::parse("window.coderabbit.opens.7"),
            Some(WindowName {
                kind: LeaseKind::coderabbit(),
                fact: WindowFact::Opens,
                epoch: Epoch(7)
            })
        );
        assert_eq!(
            WindowName::parse("window.coderabbit.quota-10.7").map(|m| m.fact),
            Some(WindowFact::Quota(10))
        );
        for other in [
            "window.coderabbit.opens",
            "window.coderabbit.closes.7",
            "lease.coderabbit.opens.7",
            "window.gpu.quota-1.7",
            "window.coderabbit.quota-1.-7",
            "window.coderabbit.quota.7",
            "window.coderabbit.quota-+1.7",
            "window.coderabbit.quota-x.7",
        ] {
            assert_eq!(WindowName::parse(other), None, "{other}");
        }
    }

    #[test]
    fn a_grant_after_the_ask_was_withdrawn_is_refused() {
        let mut asker = Asker::new(Epoch(9));
        let _ = asker.want(&stand_in());
        let _ = asker.give_back(&stand_in());
        assert_eq!(
            asker.grant("stand-in 9"),
            Err(GrantError::NotAsked(stand_in()))
        );
        assert!(!asker.holds(&stand_in()));
    }
}
