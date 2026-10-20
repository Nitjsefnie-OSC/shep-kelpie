//! A gateway's text classifier, as kelpie asks it
//!
//! A classifier is a small model the project's gateway serves behind a route of
//! its own. Kelpie sends it the text of one item and one yes/no question, and
//! the gateway answers with the probability that the answer is yes. It is no
//! model call in kelpie's accounting: it has no session, no harness and no
//! lease, it is given no repo, and what comes back is a number and not prose.
//! Kelpie asks it through this port, so a test answers it in memory and the
//! adapter holds the gateway's request shape.

use std::fmt;

use super::issue::AuthorAssociation;
use crate::forwarder::Upstream;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A classifier's answer: the probability that the answer to its question is
/// yes. Kept in millionths so kelpie's settings and state can compare it with
/// `Eq` and `Ord`, and read down to the millionth at or below the number it
/// stands for, so an answer is never raised to a threshold it was under. Held
/// to the thousandth, 0.7996 would stand at 0.8 and be read as spam at the
/// default threshold. Written as the number from 0 to 1 the gateway gave.
// wire format: changing this is a change to the state file's triage records
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(try_from = "f64")]
// schemars describes a `try_from` type by its source, so the bounds go here.
#[schemars(extend("minimum" = 0, "maximum" = 1))]
pub struct Probability(u32);

/// How many of them one is held in: a `Probability` counts this many millionths
const PARTS: u32 = 1_000_000;

impl Probability {
    /// A probability from `value` millionths, 0 to 1_000_000
    #[must_use]
    pub const fn millionths(value: u32) -> Self {
        Self(value)
    }

    /// `value` from 0 to 1, read down to the millionth at or below it, or `None`
    /// when it is not a probability at all
    #[must_use]
    pub fn new(value: f64) -> Option<Self> {
        match value.is_finite() && (0.0..=1.0).contains(&value) {
            // The cut is taken a hair above the product, because a whole
            // millionth is a hair under itself once it is a float: 0.29 times a
            // million is 290000.00000000006 for some of them and 289999.99999999994
            // for others, and a number that stands at a millionth is held as that
            // millionth.
            true => Some(Self((value * f64::from(PARTS) + 1e-9).floor() as u32)),
            false => None,
        }
    }

    /// The number it stands for, 0 to 1
    #[must_use]
    pub fn value(self) -> f64 {
        f64::from(self.0) / f64::from(PARTS)
    }
}

impl TryFrom<f64> for Probability {
    type Error = &'static str;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        match Self::new(value) {
            Some(p) => Ok(p),
            None => Err("must be a probability from 0 to 1, such as 0.8"),
        }
    }
}

impl Serialize for Probability {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f64(self.value())
    }
}

impl fmt::Display for Probability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value())
    }
}

/// What kelpie asks a classifier about one issue
#[derive(Debug)]
pub struct SpamState<'a> {
    /// The issue's title, never cut
    pub title: &'a str,
    /// The issue's body, cut to the size its research call is given
    pub body: &'a str,
    /// How the forge counts whoever opened it to the repo, or none when the
    /// listing does not say
    pub author_association: Option<AuthorAssociation>,
    /// How many whole days the author's account has existed, or none when the
    /// forge cannot say
    pub account_age_days: Option<u64>,
}

/// Why a classifier gave kelpie no probability
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassifierError {
    /// The gateway could not be reached, with why
    Unreachable(String),
    /// The gateway refused the request, with what it answered
    Refused(String),
    /// The gateway answered, but not with a probability kelpie can read
    Unreadable(String),
}

impl fmt::Display for ClassifierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreachable(why) => write!(f, "the classifier could not be reached: {why}"),
            Self::Refused(why) => write!(f, "the classifier refused the request: {why}"),
            Self::Unreadable(why) => write!(f, "the classifier's answer could not be read: {why}"),
        }
    }
}

impl core::error::Error for ClassifierError {}

/// One kind of gateway classifier, as kelpie asks it
pub trait Classifier: Send + Sync {
    /// The probability that the answer to the `spam` question about `issue` is
    /// yes
    ///
    /// `gateway` is the gateway's address, with its key where it has one, and
    /// `path` the route its classifier is served on.
    ///
    /// # Errors
    ///
    /// [`ClassifierError`] when the gateway cannot be reached, refuses the
    /// request, or answers with something that is not a probability.
    fn spam(
        &self,
        gateway: &Upstream,
        path: &str,
        issue: &SpamState<'_>,
    ) -> Result<Probability, ClassifierError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // An answer is held down to the millionth at or below it, so it is never
    // raised to a threshold it was under, and a number that stands at a whole
    // millionth is held as that millionth.
    #[test]
    fn an_answer_is_held_down_to_the_millionth_at_or_below_it() {
        assert_eq!(
            Probability::new(0.7996),
            Some(Probability::millionths(799_600)),
            "an answer just under 0.8 stays under it"
        );
        assert_eq!(
            Probability::new(0.7999996),
            Some(Probability::millionths(799_999)),
            "and one a hair under is held down too"
        );
        assert_eq!(
            Probability::new(0.29),
            Some(Probability::millionths(290_000)),
            "a whole millionth a float is a hair under is held as itself"
        );
        assert_eq!(
            Probability::new(0.8),
            Some(Probability::millionths(800_000)),
            "and one at the threshold stands at it"
        );
        assert_eq!(
            Probability::new(1.0),
            Some(Probability::millionths(1_000_000))
        );
        assert_eq!(Probability::new(0.0), Some(Probability::millionths(0)));
    }

    #[test]
    fn a_number_that_is_not_a_probability_is_no_answer() {
        assert_eq!(Probability::new(1.2), None, "above 1");
        assert_eq!(Probability::new(-0.1), None, "below 0");
        assert_eq!(Probability::new(f64::NAN), None, "not a number");
        assert_eq!(Probability::new(f64::INFINITY), None, "not finite");
    }
}
