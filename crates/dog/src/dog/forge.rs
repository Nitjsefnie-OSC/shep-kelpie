//! The login's forge hold and pace, for every runner that asks
//!
//! GitHub limits a login, and every runner on it reaches this one dog, so its
//! hold and its spacing of calls are kept here. A `forge` ask is answered with
//! one line and the connection closes, so it holds no place.

use std::sync::Arc;

use super::door::{Place, say};
use super::lock_desk;
use crate::ipc::ServerWriteHalf;
use crate::lease::door::{Answer, Ask, ForgeAsk};
use crate::metrics::Recorder;
use crate::ports::{LocalPace, Pace, Pass, Timestamp};

// What `ask` is told of the forge at `now`, with the hold's numbers sent.
pub(super) fn answer(
    pace: &LocalPace,
    metrics: &Recorder,
    ask: ForgeAsk,
    now: Timestamp,
) -> Answer {
    let answer = match ask {
        ForgeAsk::Pass { project } => match pace.pass(&project, now) {
            Pass::Go => Answer::Go,
            Pass::Wait(seconds) => Answer::Wait(seconds),
            Pass::Held(until) => Answer::Held(until.0),
        },
        ForgeAsk::Held => pace
            .held(now)
            .map_or(Answer::Go, |until| Answer::Held(until.0)),
        ForgeAsk::Hold(until) => {
            // Every runner on the login reports the same used-up answer, and
            // only the one that finds nothing held begins a hold.
            let began = pace.held(now).is_none();
            pace.hold(Timestamp(until));
            let held = pace.held(now);
            if began && held.is_some() {
                metrics.forge_hold();
            }
            held.map_or(Answer::Go, |until| Answer::Held(until.0))
        }
    };
    metrics.forge_held(matches!(answer, Answer::Held(_)));
    answer
}

// Sends whether the forge is held at `now`: a hold ends without anyone telling the dog.
pub(super) fn publish(pace: &LocalPace, metrics: &Recorder, now: Timestamp) {
    metrics.forge_held(pace.held(now).is_some());
}

// Answers a runner's `forge` ask and ends the visit.
pub(super) async fn visit(place: &Place, ask: Ask, write: &mut ServerWriteHalf) {
    let Some(forge) = ask.forge else {
        let why = "a forge ask says what it wants of the forge".to_owned();
        return say(write, &Answer::Error(why)).await;
    };
    let (pace, metrics, now) = {
        let kept = lock_desk(&place.desk);
        (
            kept.pace.clone(),
            Arc::clone(&kept.desk.metrics),
            kept.desk.book.now(),
        )
    };
    say(write, &answer(&pace, &metrics, forge, now)).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::CallPacer;
    use crate::test::RecordedSink;

    const T: Timestamp = Timestamp(1_790_000_000);

    fn pass(project: &str) -> ForgeAsk {
        ForgeAsk::Pass {
            project: project.into(),
        }
    }

    #[test]
    fn a_hold_one_runner_reports_is_told_to_the_next_that_asks() {
        let pace = LocalPace::new();
        let recorder = RecordedSink::default().recorder();

        assert_eq!(answer(&pace, &recorder, ForgeAsk::Held, T), Answer::Go);
        assert_eq!(
            answer(&pace, &recorder, ForgeAsk::Hold(T.0 + 90), T),
            Answer::Held(T.0 + 90)
        );

        assert_eq!(
            answer(&pace, &recorder, pass("golbat"), T),
            Answer::Held(T.0 + 90)
        );
        assert_eq!(
            answer(&pace, &recorder, ForgeAsk::Held, T),
            Answer::Held(T.0 + 90)
        );
        let later = Timestamp(T.0 + 90);
        assert_eq!(answer(&pace, &recorder, pass("golbat"), later), Answer::Go);
    }

    #[test]
    fn two_runners_asking_to_go_share_the_minutes_cap() {
        let pace = LocalPace::with(CallPacer::new(2, 5));
        let recorder = RecordedSink::default().recorder();

        assert_eq!(answer(&pace, &recorder, pass("koji"), T), Answer::Go);
        assert_eq!(answer(&pace, &recorder, pass("golbat"), T), Answer::Go);
        assert_eq!(answer(&pace, &recorder, pass("koji"), T), Answer::Wait(60));
    }

    fn later(seconds: u64) -> Timestamp {
        Timestamp(T.0 + seconds)
    }

    const HOLDS: &str = "kelpie_forge_holds_total";
    const HELD: &str = "kelpie_forge_held";
    const LOGIN: [(&str, &str); 1] = [("login_kind", "login")];

    #[test]
    fn a_hold_a_runner_reports_counts_once_and_reads_held_until_it_ends() {
        let (pace, samples) = (LocalPace::new(), RecordedSink::default());
        let recorder = samples.recorder();
        answer(&pace, &recorder, ForgeAsk::Held, T);
        assert_eq!(samples.value(HELD, &[]), Some(0.0));
        assert_eq!(samples.value(HOLDS, &LOGIN), None);

        answer(&pace, &recorder, ForgeAsk::Hold(T.0 + 90), T);
        assert_eq!(samples.value(HELD, &[]), Some(1.0));
        assert_eq!(samples.value(HOLDS, &LOGIN), Some(1.0));

        // Another runner reports the same used-up answer, and a later reset.
        answer(&pace, &recorder, ForgeAsk::Hold(T.0 + 90), later(5));
        answer(&pace, &recorder, ForgeAsk::Hold(T.0 + 120), later(10));
        answer(&pace, &recorder, pass("golbat"), later(20));
        assert_eq!(samples.value(HOLDS, &LOGIN), Some(1.0));

        publish(&pace, &recorder, later(119));
        assert_eq!(samples.value(HELD, &[]), Some(1.0));
        publish(&pace, &recorder, later(120));
        assert_eq!(samples.value(HELD, &[]), Some(0.0));

        answer(&pace, &recorder, ForgeAsk::Hold(T.0 + 300), later(130));
        assert_eq!(samples.value(HOLDS, &LOGIN), Some(2.0));
    }

    #[test]
    fn a_hold_already_over_is_no_hold() {
        let (pace, samples) = (LocalPace::new(), RecordedSink::default());

        answer(&pace, &samples.recorder(), ForgeAsk::Hold(T.0 - 1), T);

        assert_eq!(samples.value(HELD, &[]), Some(0.0));
        assert_eq!(samples.value(HOLDS, &LOGIN), None);
    }

    #[test]
    fn a_runner_let_go_reads_the_forge_free() {
        let (pace, samples) = (LocalPace::new(), RecordedSink::default());
        let recorder = samples.recorder();
        answer(&pace, &recorder, ForgeAsk::Hold(T.0 + 30), T);

        let told = answer(&pace, &recorder, pass("koji"), later(30));

        assert_eq!(told, Answer::Go);
        assert_eq!(samples.value(HELD, &[]), Some(0.0));
    }
}
