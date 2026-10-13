//! The login's forge hold and pace, for every runner that asks
//!
//! GitHub limits a login, and every runner on it reaches this one dog, so its
//! hold and its spacing of calls are kept here. A `forge` ask is answered with
//! one line and the connection closes, so it holds no place.

use tokio::net::unix::OwnedWriteHalf;

use super::door::{Place, say};
use super::lock_desk;
use crate::lease::door::{Answer, Ask, ForgeAsk};
use crate::ports::{LocalPace, Pace, Pass, Timestamp};

// What `ask` is told of the forge at `now`.
pub(super) fn answer(pace: &LocalPace, ask: ForgeAsk, now: Timestamp) -> Answer {
    match ask {
        ForgeAsk::Pass { project } => match pace.pass(&project, now) {
            Pass::Go => Answer::Go,
            Pass::Wait(seconds) => Answer::Wait(seconds),
            Pass::Held(until) => Answer::Held(until.0),
        },
        ForgeAsk::Held => pace
            .held(now)
            .map_or(Answer::Go, |until| Answer::Held(until.0)),
        ForgeAsk::Hold(until) => {
            pace.hold(Timestamp(until));
            pace.held(now)
                .map_or(Answer::Go, |until| Answer::Held(until.0))
        }
    }
}

// Answers a runner's `forge` ask and ends the visit.
pub(super) async fn visit(place: &Place, ask: Ask, write: &mut OwnedWriteHalf) {
    let Some(forge) = ask.forge else {
        let why = "a forge ask says what it wants of the forge".to_owned();
        return say(write, &Answer::Error(why)).await;
    };
    let (pace, now) = {
        let kept = lock_desk(&place.desk);
        (kept.pace.clone(), kept.desk.book.now())
    };
    say(write, &answer(&pace, forge, now)).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::CallPacer;

    const T: Timestamp = Timestamp(1_790_000_000);

    fn pass(project: &str) -> ForgeAsk {
        ForgeAsk::Pass {
            project: project.into(),
        }
    }

    #[test]
    fn a_hold_one_runner_reports_is_told_to_the_next_that_asks() {
        let pace = LocalPace::new();

        assert_eq!(answer(&pace, ForgeAsk::Held, T), Answer::Go);
        assert_eq!(
            answer(&pace, ForgeAsk::Hold(T.0 + 90), T),
            Answer::Held(T.0 + 90)
        );

        assert_eq!(answer(&pace, pass("golbat"), T), Answer::Held(T.0 + 90));
        assert_eq!(answer(&pace, ForgeAsk::Held, T), Answer::Held(T.0 + 90));
        let later = Timestamp(T.0 + 90);
        assert_eq!(answer(&pace, pass("golbat"), later), Answer::Go);
    }

    #[test]
    fn two_runners_asking_to_go_share_the_minutes_cap() {
        let pace = LocalPace::with(CallPacer::new(2, 5));

        assert_eq!(answer(&pace, pass("koji"), T), Answer::Go);
        assert_eq!(answer(&pace, pass("golbat"), T), Answer::Go);
        assert_eq!(answer(&pace, pass("koji"), T), Answer::Wait(60));
    }
}
