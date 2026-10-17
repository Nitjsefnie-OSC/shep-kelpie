//! The classifier stand-in: what each check was asked, and what it answers
//!
//! It answers in memory, so a test of the spam check never reaches a gateway.
//! It starts by answering 0, which is under every threshold, and a test sets
//! the probability it wants, or makes it fail.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use crate::forwarder::Upstream;
use crate::ports::{AuthorAssociation, Classifier, ClassifierError, Probability, SpamState};

/// What one check was asked about, and where it was asked
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SeenAsk {
    /// The title it was given
    pub title: String,
    /// The body it was given, which is where a test sees the cut
    pub body: String,
    /// How the forge counts the author, as it was given
    pub author_association: Option<AuthorAssociation>,
    /// The author's account age in days, as it was given
    pub account_age_days: Option<u64>,
    /// The gateway's `host:port`, such as `gpu:8700`
    pub address: String,
    /// The route it was asked on
    pub path: String,
}

#[derive(Debug, Default)]
struct Book {
    /// What each check answers, in order, and what the last of them answers
    /// once they run out
    answers: VecDeque<Probability>,
    /// What every check answers instead, while it is set
    failure: Option<ClassifierError>,
    asks: Vec<SeenAsk>,
}

/// A classifier that answers what a test sets
#[derive(Debug, Clone)]
pub(crate) struct FakeClassifier {
    book: Arc<Mutex<Book>>,
}

impl FakeClassifier {
    /// One that answers 0 to every check
    pub(crate) fn new() -> Self {
        Self {
            book: Arc::new(Mutex::new(Book::default())),
        }
    }

    /// Every check answers `probability`
    pub(crate) fn says(&self, probability: f64) {
        self.answers(&[probability]);
    }

    /// Each check answers the next of `probabilities`, and the last one after them
    pub(crate) fn answers(&self, probabilities: &[f64]) {
        let mut book = self.book.lock().unwrap();
        book.failure = None;
        book.answers = probabilities
            .iter()
            .filter_map(|&p| Probability::new(p))
            .collect();
    }

    /// Every check fails with `error`
    pub(crate) fn fails(&self, error: ClassifierError) {
        self.book.lock().unwrap().failure = Some(error);
    }

    /// What each check was asked, in order
    pub(crate) fn asks(&self) -> Vec<SeenAsk> {
        self.book.lock().unwrap().asks.clone()
    }

    /// How many checks it answered
    pub(crate) fn count(&self) -> usize {
        self.book.lock().unwrap().asks.len()
    }
}

impl Classifier for FakeClassifier {
    fn spam(
        &self,
        gateway: &Upstream,
        path: &str,
        issue: &SpamState<'_>,
    ) -> Result<Probability, ClassifierError> {
        let mut book = self.book.lock().unwrap();
        book.asks.push(SeenAsk {
            title: issue.title.to_owned(),
            body: issue.body.to_owned(),
            author_association: issue.author_association,
            account_age_days: issue.account_age_days,
            address: gateway.address().to_owned(),
            path: path.to_owned(),
        });
        if let Some(error) = &book.failure {
            return Err(error.clone());
        }
        // The last answer given stands for every check after it.
        Ok(match book.answers.len() {
            0 => Probability::millionths(0),
            1 => book.answers[0],
            _ => book
                .answers
                .pop_front()
                .unwrap_or(Probability::millionths(0)),
        })
    }
}
