//! The runner's lease totals across a handover

use super::*;
use crate::adapters::ShepLeases;
use crate::lease::saved::{
    BookFile, SavedBook, SavedHeld, SavedHolder, SavedLease, SavedRun, SavedTotals,
};
use crate::lease::wire::grant_params;
use crate::lease::{Epoch, LeaseKind};
use crate::ports::Leases;
use crate::ports::Timestamp;
use crate::runner::ProjectName;
use crate::test::{DogStandIn, short_tempdir};

const EPOCH: Epoch = Epoch(4242);

fn stand_in() -> LeaseKind {
    LeaseKind::try_from("stand-in").unwrap()
}

// A runner of run `EPOCH` that asked for the stand-in and gave it back
// twice, with the dog having heard each.
fn two_rounds_in(dog: &Arc<DogStandIn>, book: &BookFile) -> ShepLeases {
    let leases = ShepLeases::new(dog.clone(), Asker::new(EPOCH), book.clone());
    for _ in 0..2 {
        leases.want(&stand_in());
        leases.give_back(&stand_in());
    }
    leases
}

// What the old image writes and the new image reads.
fn across_the_wire(carry: &Carry) -> Carry {
    serde_json::from_slice(&serde_json::to_vec(carry).unwrap()).unwrap()
}

fn book_in(folder: &Path) -> BookFile {
    BookFile::new(folder.join("book.json"))
}

#[test]
fn the_lease_totals_cross_a_handover() {
    let (dog, folder) = (Arc::new(DogStandIn::default()), short_tempdir());
    let book = book_in(folder.path());
    let old = two_rounds_in(&dog, &book);

    let carry = across_the_wire(&Carry {
        turns: Vec::new(),
        leases: Some(old.keep()),
    });
    let new = ShepLeases::new(dog.clone(), carry.asker(EPOCH, "koji", &book), book);
    new.want(&stand_in());

    assert!(
        dog.asking(&stand_in(), EPOCH),
        "the ask counts on from the old image's two, so the dog hears it"
    );
}

#[test]
fn what_the_runner_holds_crosses_a_handover() {
    let (dog, folder) = (Arc::new(DogStandIn::default()), short_tempdir());
    let book = book_in(folder.path());
    let old = ShepLeases::new(dog.clone(), Asker::new(EPOCH), book.clone());
    old.want(&stand_in());
    old.grant(&grant_params(&stand_in(), EPOCH)).unwrap();

    let carry = across_the_wire(&Carry {
        turns: Vec::new(),
        leases: Some(old.keep()),
    });
    let new = ShepLeases::new(dog.clone(), carry.asker(EPOCH, "koji", &book), book);

    assert!(new.holds(&stand_in()));
    new.give_back(&stand_in());
    assert!(!dog.asking(&stand_in(), EPOCH), "the return is heard");
}

#[test]
fn a_handover_from_a_build_without_totals_starts_from_the_dogs_book() {
    let (dog, folder) = (Arc::new(DogStandIn::default()), short_tempdir());
    let book = book_in(folder.path());
    // The old image's runner had asked twice, and the dog had saved that.
    let _ = two_rounds_in(&dog, &book);
    let totals = SavedTotals {
        kind: stand_in(),
        want: 2,
        give_back: 2,
    };
    let run = SavedRun {
        sheep: "koji".into(),
        epoch: EPOCH,
        totals: vec![totals],
    };
    book.save(&SavedBook::new(Vec::new(), vec![run], Vec::new()))
        .unwrap();

    // A format 4 `Carry`: the turns alone.
    let carry: Carry = serde_json::from_value(serde_json::json!({ "turns": [] })).unwrap();
    assert!(
        <Carry as State>::reads(4),
        "this build reads the old format"
    );
    let new = ShepLeases::new(dog.clone(), carry.asker(EPOCH, "koji", &book), book);
    new.want(&stand_in());

    assert!(
        dog.asking(&stand_in(), EPOCH),
        "the ask counts on from the book's two, so the dog hears it"
    );
}

#[test]
fn a_book_that_knows_another_run_starts_the_totals_at_0() {
    let folder = short_tempdir();
    let book = book_in(folder.path());
    let run = SavedRun {
        sheep: "koji".into(),
        epoch: Epoch(1),
        totals: vec![SavedTotals {
            kind: stand_in(),
            want: 5,
            give_back: 5,
        }],
    };
    book.save(&SavedBook::new(Vec::new(), vec![run], Vec::new()))
        .unwrap();
    let carry: Carry = serde_json::from_value(serde_json::json!({ "turns": [] })).unwrap();

    let mut asker = carry.asker(EPOCH, "koji", &book);

    assert_eq!(
        asker.want(&stand_in()),
        ("lease.stand-in.want.4242".into(), 1.0)
    );
}

#[test]
fn a_lease_the_dogs_book_gives_this_run_is_held_after_a_handover_from_a_build_without_totals() {
    let folder = short_tempdir();
    let book = book_in(folder.path());
    let run = SavedRun {
        sheep: "koji".into(),
        epoch: EPOCH,
        totals: vec![SavedTotals {
            kind: stand_in(),
            want: 1,
            give_back: 0,
        }],
    };
    let held = SavedHeld {
        holder: SavedHolder::Runner {
            project: ProjectName::try_from("koji").unwrap(),
            epoch: EPOCH,
        },
        since: Timestamp(1_000),
    };
    let lease = SavedLease {
        kind: stand_in(),
        held: Some(held),
        queue: Vec::new(),
        window: None,
    };
    book.save(&SavedBook::new(vec![lease], vec![run], Vec::new()))
        .unwrap();
    let carry: Carry = serde_json::from_value(serde_json::json!({ "turns": [] })).unwrap();

    let asker = carry.asker(EPOCH, "koji", &book);

    assert!(asker.holds(&stand_in()));
}

#[test]
fn a_grant_that_landed_after_the_totals_were_taken_is_held_in_the_new_image() {
    let (dog, folder) = (Arc::new(DogStandIn::default()), short_tempdir());
    let book = book_in(folder.path());
    let old = ShepLeases::new(dog.clone(), Asker::new(EPOCH), book.clone());
    old.want(&stand_in());
    let carry = across_the_wire(&Carry {
        turns: Vec::new(),
        leases: Some(old.keep()),
    });
    // The dog's grant reached the old image after that, and the book has it.
    let held = SavedHeld {
        holder: SavedHolder::Runner {
            project: ProjectName::try_from("koji").unwrap(),
            epoch: EPOCH,
        },
        since: Timestamp(1_000),
    };
    let lease = SavedLease {
        kind: stand_in(),
        held: Some(held),
        queue: Vec::new(),
        window: None,
    };
    let run = SavedRun {
        sheep: "koji".into(),
        epoch: EPOCH,
        totals: vec![SavedTotals {
            kind: stand_in(),
            want: 1,
            give_back: 0,
        }],
    };
    book.save(&SavedBook::new(vec![lease], vec![run], Vec::new()))
        .unwrap();

    let new = ShepLeases::new(dog.clone(), carry.asker(EPOCH, "koji", &book), book);

    assert!(new.holds(&stand_in()));
}

// The dog's book with run `EPOCH` of "koji" at these totals for the stand-in,
// and the stand-in held by it.
fn save_book_with_the_run_holding(book: &BookFile, want: u64, give_back: u64) {
    let held = SavedHeld {
        holder: SavedHolder::Runner {
            project: ProjectName::try_from("koji").unwrap(),
            epoch: EPOCH,
        },
        since: Timestamp(1_000),
    };
    let lease = SavedLease {
        kind: stand_in(),
        held: Some(held),
        queue: Vec::new(),
        window: None,
    };
    let totals = SavedTotals {
        kind: stand_in(),
        want,
        give_back,
    };
    let run = SavedRun {
        sheep: "koji".into(),
        epoch: EPOCH,
        totals: vec![totals],
    };
    book.save(&SavedBook::new(vec![lease], vec![run], Vec::new()))
        .unwrap();
}

#[test]
fn a_book_that_has_not_heard_the_runners_latest_totals_gives_it_no_hold() {
    let (dog, folder) = (Arc::new(DogStandIn::default()), short_tempdir());
    let book = book_in(folder.path());
    let old = ShepLeases::new(dog.clone(), Asker::new(EPOCH), book.clone());
    old.want(&stand_in());
    old.give_back(&stand_in());
    old.want(&stand_in());
    let carry = across_the_wire(&Carry {
        turns: Vec::new(),
        leases: Some(old.keep()),
    });
    // The dog has heard the first ask only, and still has the run holding.
    save_book_with_the_run_holding(&book, 1, 0);

    let new = ShepLeases::new(dog.clone(), carry.asker(EPOCH, "koji", &book), book);

    assert!(!new.holds(&stand_in()));
}

#[test]
fn a_grant_the_dog_saved_before_the_handler_was_up_is_held_when_the_runner_looks_again() {
    let (dog, folder) = (Arc::new(DogStandIn::default()), short_tempdir());
    let book = book_in(folder.path());
    let old = ShepLeases::new(dog.clone(), Asker::new(EPOCH), book.clone());
    old.want(&stand_in());
    let carry = across_the_wire(&Carry {
        turns: Vec::new(),
        leases: Some(old.keep()),
    });
    let new = ShepLeases::new(dog.clone(), carry.asker(EPOCH, "koji", &book), book.clone());
    assert!(!new.holds(&stand_in()), "the book had no grant yet");

    // The dog granted it after the new image read the book, with no handler up.
    save_book_with_the_run_holding(&book, 1, 0);
    new.reconcile("koji");

    assert!(new.holds(&stand_in()));
}
