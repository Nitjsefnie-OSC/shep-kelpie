//! What the dog does with the shepherd's bus events, without waiting on anything
//!
//! The dog reads one stream, and a read of the flock or a grant sent while
//! it waits is a run of events the stream drops. So [`Bus::hear`] answers at
//! once, and when the flock must be read it says so through [`Bus::read_at`]
//! for the loop to send, with the reads one at a time and [`RESYNC_GAP`] apart.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use shep_client::Lagged;
use shep_client::shep_core::protocol::{BusEvent, ChildMessage, ProcessEventKind};
use tokio::time::Instant;

use super::desk::Delivery;
use super::{Flock, Kept, lock_desk};

/// The least time between one read of the flock coming back and the next going out
pub const RESYNC_GAP: Duration = Duration::from_secs(5);

/// The dog's view of the bus: who each runner is, and whether the flock needs reading
pub struct Bus {
    names: HashMap<u32, String>,
    // What runners with no name yet told the dog, in the order heard
    unnamed: Vec<(u32, String, f64)>,
    // Events were missed or a runner is unknown since the last read went out
    stale: bool,
    reading: bool,
    ready_at: Instant,
}

impl Bus {
    /// A bus that knows no runner, and may read the flock at `now`
    pub fn new(now: Instant) -> Self {
        Self {
            names: HashMap::new(),
            unnamed: Vec::new(),
            stale: false,
            reading: false,
            ready_at: now,
        }
    }

    /// What the dog grants for `event`
    ///
    /// Events missed count in `kelpie_dog_bus_missed_total` and leave the flock
    /// to be read. A note from a runner not yet named waits for that read.
    pub fn hear(&mut self, event: Result<BusEvent, Lagged>, desk: &Mutex<Kept>) -> Vec<Delivery> {
        match event {
            Err(lagged) => {
                println!("missed bus events ({lagged:?}): checking every runner");
                self.missed(lagged.count, desk);
                Vec::new()
            }
            Ok(BusEvent::Dropped { count }) => {
                println!("the bus dropped {count} events: checking every runner");
                self.missed(count, desk);
                Vec::new()
            }
            Ok(BusEvent::Channel { id, message }) => {
                let Some((name, value)) = told(message) else {
                    return Vec::new();
                };
                if self.names.contains_key(&id) {
                    self.heard(id, &name, value, desk)
                } else {
                    self.unnamed.push((id, name, value));
                    self.stale = true;
                    Vec::new()
                }
            }
            Ok(BusEvent::Process { event, info, .. }) => {
                self.names.insert(info.id, info.name.clone());
                let live = match event {
                    ProcessEventKind::Restart => info.pid,
                    ProcessEventKind::Exit
                    | ProcessEventKind::Stop
                    | ProcessEventKind::Errored
                    | ProcessEventKind::Delete => None,
                    _ => return Vec::new(),
                };
                lock_desk(desk).change(|d| d.runner_is(&info.name, live))
            }
            Ok(_) => Vec::new(),
        }
    }

    /// When the flock may be read, or `None` while none needs it or a read is out
    pub fn read_at(&self) -> Option<Instant> {
        (self.stale && !self.reading).then_some(self.ready_at)
    }

    /// Notes that the read `read_at` named has gone out
    pub fn reading(&mut self) {
        self.stale = false;
        self.reading = true;
    }

    /// Takes the flock as read at `now`: the names, the runners to reclaim,
    /// and what the notes that waited for a name grant
    pub fn read(&mut self, listed: Flock, now: Instant, desk: &Mutex<Kept>) -> Vec<Delivery> {
        self.reading = false;
        self.ready_at = now + RESYNC_GAP;
        self.names = listed.names;
        let mut grants = lock_desk(desk).change(|d| d.resync(&listed.live));
        for (id, name, value) in std::mem::take(&mut self.unnamed) {
            if self.names.contains_key(&id) {
                grants.extend(self.heard(id, &name, value, desk));
            }
        }
        grants
    }

    fn missed(&mut self, count: u64, desk: &Mutex<Kept>) {
        lock_desk(desk).desk.metrics.bus_missed(count);
        self.stale = true;
    }

    fn heard(&self, id: u32, name: &str, value: f64, desk: &Mutex<Kept>) -> Vec<Delivery> {
        let Some(sheep) = self.names.get(&id) else {
            return Vec::new();
        };
        let heard = lock_desk(desk).change(|d| d.heard(sheep, name, value));
        if let Some(line) = heard.ignored {
            println!("{line}");
        }
        heard.grants
    }
}

// The name and value of what a runner told the dog on its channel, or
// `None` for a message that is not for the dog.
fn told(message: ChildMessage) -> Option<(String, f64)> {
    match message {
        ChildMessage::Note { name, value } => Some((name, value)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::home::ProjectName;
    use crate::lease::wire::Asker;
    use crate::lease::{Epoch, LeaseKind};
    use crate::review_bot::Reviewers;
    use crate::test::{EPOCH, FakeClock, RecordedSink};

    const KOJI: u32 = 7;
    const MOSS: u32 = 9;

    struct Dog {
        desk: Mutex<Kept>,
        sink: RecordedSink,
        bus: Bus,
        now: Instant,
        _dir: tempfile::TempDir,
    }

    // A dog that has read a flock of koji alone.
    fn dog() -> Dog {
        let dir = tempfile::tempdir().unwrap();
        let sink = RecordedSink::default();
        let kept = super::super::open(
            crate::lease::saved::BookFile::new(dir.path().join("book.json")),
            Box::new(FakeClock::at(EPOCH)),
            Reviewers::default(),
            sink.recorder(),
        );
        let now = Instant::now();
        let mut dog = Dog {
            desk: Mutex::new(kept),
            sink,
            bus: Bus::new(now),
            now,
            _dir: dir,
        };
        dog.read(&[(KOJI, "koji")]);
        dog
    }

    impl Dog {
        // The flock as the shepherd lists it, every runner online.
        fn listing(runners: &[(u32, &str)]) -> Flock {
            Flock {
                names: runners
                    .iter()
                    .map(|(id, n)| (*id, (*n).to_owned()))
                    .collect(),
                live: runners
                    .iter()
                    .map(|(id, n)| ((*n).to_owned(), *id))
                    .collect(),
            }
        }

        // A read of the flock that comes back after a second.
        fn read(&mut self, runners: &[(u32, &str)]) -> Vec<Delivery> {
            self.bus.reading();
            self.now += Duration::from_secs(1);
            self.bus.read(Self::listing(runners), self.now, &self.desk)
        }

        fn hear(&mut self, event: Result<BusEvent, Lagged>) -> Vec<Delivery> {
            self.bus.hear(event, &self.desk)
        }

        fn lags(&mut self, count: u64) {
            self.hear(Err(Lagged { count }));
        }

        fn missed(&self) -> Option<f64> {
            self.sink.value("kelpie_dog_bus_missed_total", &[])
        }
    }

    fn kind(name: &str) -> LeaseKind {
        LeaseKind::try_from(name).unwrap()
    }

    fn ask(id: u32, kind: &LeaseKind) -> Result<BusEvent, Lagged> {
        let (name, value) = Asker::new(Epoch(101)).want(kind);
        Ok(BusEvent::Channel {
            id,
            message: ChildMessage::Note { name, value },
        })
    }

    fn grant(project: &str, kind: &LeaseKind) -> Delivery {
        Delivery {
            project: ProjectName::try_from(project).unwrap(),
            kind: kind.clone(),
            epoch: Epoch(101),
        }
    }

    #[test]
    fn a_runner_s_ask_sent_as_a_note_is_granted() {
        let mut dog = dog();
        let stand_in = kind("stand-in");
        assert_eq!(dog.hear(ask(KOJI, &stand_in)), [grant("koji", &stand_in)]);
    }

    #[test]
    fn a_metric_sample_is_ignored() {
        let mut dog = dog();
        let stand_in = kind("stand-in");
        let (name, value) = Asker::new(Epoch(101)).want(&stand_in);
        let sample = ChildMessage::Metric {
            name,
            value,
            metric_type: None,
            labels: BTreeMap::new(),
            help: None,
        };
        let heard = dog.hear(Ok(BusEvent::Channel {
            id: KOJI,
            message: sample,
        }));
        assert_eq!(heard, []);
        assert_eq!(dog.bus.read_at(), None);
    }

    #[test]
    fn a_reply_on_the_channel_grants_nothing() {
        let mut dog = dog();
        let reply = ChildMessage::ActionReply {
            action: "lease.stand-in.want.101".into(),
            body: "1".into(),
            id: None,
        };
        let message = Ok(BusEvent::Channel {
            id: KOJI,
            message: reply,
        });
        assert_eq!(dog.hear(message), []);
    }

    #[test]
    fn a_burst_of_lagged_events_asks_for_one_read() {
        let mut dog = dog();
        let first = dog.now;
        assert_eq!(dog.bus.read_at(), None, "nothing missed yet");
        for _ in 0..50 {
            dog.lags(3);
        }
        assert_eq!(dog.bus.read_at(), Some(dog.bus.ready_at));
        assert!(
            dog.bus.ready_at <= first + RESYNC_GAP,
            "the first is not held back"
        );

        dog.bus.reading();
        dog.lags(3);
        dog.hear(Ok(BusEvent::Dropped { count: 2 }));
        assert_eq!(dog.bus.read_at(), None, "one read at a time");
    }

    #[test]
    fn a_read_is_not_followed_by_another_within_the_gap() {
        let mut dog = dog();
        dog.lags(3);
        dog.bus.reading();
        dog.lags(3);
        let back = dog.now + Duration::from_secs(1);
        dog.bus
            .read(Dog::listing(&[(KOJI, "koji")]), back, &dog.desk);
        assert_eq!(
            dog.bus.read_at(),
            Some(back + RESYNC_GAP),
            "what was missed meanwhile is read once the gap has passed"
        );
    }

    #[test]
    fn a_read_with_nothing_missed_since_asks_for_no_more() {
        let mut dog = dog();
        dog.lags(3);
        dog.read(&[(KOJI, "koji")]);
        assert_eq!(dog.bus.read_at(), None);
    }

    #[test]
    fn the_events_missed_are_counted_whether_lagged_or_dropped() {
        let mut dog = dog();
        assert_eq!(dog.missed(), None);
        dog.lags(3);
        dog.lags(4);
        dog.hear(Ok(BusEvent::Dropped { count: 5 }));
        assert_eq!(dog.missed(), Some(12.0));
    }

    #[test]
    fn a_note_from_an_unnamed_runner_waits_for_the_read_and_others_are_still_heard() {
        let mut dog = dog();
        let (stand_in, other) = (kind("stand-in"), kind("other"));
        assert_eq!(dog.hear(ask(MOSS, &other)), [], "moss has no name yet");
        assert!(dog.bus.read_at().is_some(), "so the flock is read");

        dog.bus.reading();
        assert_eq!(
            dog.hear(ask(KOJI, &stand_in)),
            [grant("koji", &stand_in)],
            "koji is heard while the read is out"
        );

        let back = dog.read(&[(KOJI, "koji"), (MOSS, "moss")]);
        assert_eq!(back, [grant("moss", &other)]);
    }

    #[test]
    fn a_note_from_a_runner_the_flock_does_not_list_is_dropped() {
        let mut dog = dog();
        dog.hear(ask(MOSS, &kind("other")));
        assert_eq!(dog.read(&[(KOJI, "koji")]), []);
        assert_eq!(dog.read(&[(KOJI, "koji"), (MOSS, "moss")]), []);
    }
}
