//! Rulings saved under counter ids, renamed by the id they are named by now
//!
//! Before version 38 a ruling's id came from a counter shared by every
//! project. A ruling's id is now its pull request's number, or its ticket's
//! id before there is one, so a state file from before is renamed as it
//! loads. A file saved after is already named so and loads as it is.

use std::collections::{BTreeMap, BTreeSet};

use super::{ProjectState, Ruling};
use crate::ticket::TicketId;
use crate::work_item::Phase;

/// The counter ids a file's rulings were renamed from, by the id each is
/// named by now
///
/// Not part of the state: it is never saved, and two states are equal
/// whatever it holds.
#[derive(Debug, Clone, Default)]
pub(super) struct Renamed(BTreeMap<TicketId, TicketId>);

impl PartialEq for Renamed {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for Renamed {}

impl ProjectState {
    /// The ids rulings were renamed to as the file loaded, by the counter id
    /// each was saved under, taken: asked a second time it is empty
    ///
    /// Only the runner that loaded the file has them, so an answer left
    /// under an old id is read once, by that runner's first pass, and no old
    /// id works after.
    pub fn take_renamed(&mut self) -> BTreeMap<TicketId, TicketId> {
        std::mem::take(&mut self.renamed.0)
    }

    // Names each ruling by its id. The work item parked on it, its
    // posted comment and what is read from its thread follow the new name.
    pub(super) fn rulings_numbered(mut self) -> Self {
        let parked: BTreeSet<TicketId> = self
            .work_items
            .iter()
            .filter_map(|item| match item.phase {
                Phase::Ruling { id } => Some(id),
                _ => None,
            })
            .collect();
        let before: BTreeSet<TicketId> = self.rulings.iter().map(|r| r.id).collect();
        let rulings = std::mem::take(&mut self.rulings);
        let kept = numbered(rulings, &parked);
        let dropped: Vec<TicketId> = {
            let after: BTreeSet<TicketId> = kept.iter().map(|(old, _)| *old).collect();
            before.difference(&after).copied().collect()
        };
        let renames: BTreeMap<TicketId, TicketId> = kept
            .iter()
            .filter(|(old, r)| *old != r.id)
            .map(|(old, r)| (*old, r.id))
            .collect();
        self.rulings = kept.into_iter().map(|(_, r)| r).collect();
        if renames.is_empty() && dropped.is_empty() {
            return self;
        }
        let new = |id: &TicketId| renames.get(id).copied().unwrap_or(*id);
        for item in &mut self.work_items {
            if let Phase::Ruling { id } = &mut item.phase {
                *id = new(id);
            }
        }
        // What a dropped duplicate had posted or been read for goes with it.
        for id in &dropped {
            self.heard.remove(id);
            self.unmentioned.remove(id);
        }
        self.heard = std::mem::take(&mut self.heard)
            .into_iter()
            .map(|(id, heard)| (new(&id), heard))
            .collect();
        self.unmentioned = std::mem::take(&mut self.unmentioned)
            .into_iter()
            .map(|(id, comment)| (new(&id), comment))
            .collect();
        self.renamed = Renamed(renames);
        self
    }
}

// `rulings` named by their numbers, each with the id it was saved under. A
// work item parks on one ruling at a time, so when several are on a number,
// one is waited on and the others are left by an older kelpie: the one a work
// item is parked on, or else the last, takes the number, and the others keep
// their ids, which still tell them apart in a thread. One whose id is another's
// number cannot, and goes.
fn numbered(rulings: Vec<Ruling>, parked: &BTreeSet<TicketId>) -> Vec<(TicketId, Ruling)> {
    let mut winners: BTreeMap<TicketId, usize> = BTreeMap::new();
    for (at, ruling) in rulings.iter().enumerate() {
        let held = winners
            .get(&ruling.number())
            .is_some_and(|&won| parked.contains(&rulings[won].id) && !parked.contains(&ruling.id));
        if !held {
            winners.insert(ruling.number(), at);
        }
    }
    let won: BTreeSet<usize> = winners.values().copied().collect();
    let taken: BTreeSet<TicketId> = winners.keys().copied().collect();
    rulings
        .into_iter()
        .enumerate()
        .filter_map(|(at, ruling)| {
            let old = ruling.id;
            if won.contains(&at) {
                let id = ruling.number();
                return Some((old, Ruling { id, ..ruling }));
            }
            (!taken.contains(&old)).then_some((old, ruling))
        })
        .collect()
}
