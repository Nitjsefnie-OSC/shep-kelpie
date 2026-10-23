//! The account's rate limits, from `gh api rate_limit`, which counts
//! against none of them

use serde::Deserialize;

use super::{gh, unreadable};
use crate::ports::{ForgeError, RateBucket, RateLimits, Timestamp};

/// The login's `core` and `graphql` limits as GitHub counts them now
pub(super) fn rate_limit() -> Result<RateLimits, ForgeError> {
    parse_rate_limit(&gh(&["api", "rate_limit"])?)
}

fn parse_rate_limit(stdout: &[u8]) -> Result<RateLimits, ForgeError> {
    #[derive(Deserialize)]
    struct Limits {
        resources: Resources,
    }
    #[derive(Deserialize)]
    struct Resources {
        core: Limit,
        graphql: Limit,
    }
    #[derive(Deserialize)]
    struct Limit {
        used: u64,
        remaining: u64,
        reset: u64,
    }
    let bucket = |limit: Limit| RateBucket {
        used: limit.used,
        remaining: limit.remaining,
        reset: Timestamp(limit.reset),
    };
    let limits: Limits = serde_json::from_slice(stdout).map_err(|_| unreadable(stdout))?;
    Ok(RateLimits {
        core: bucket(limits.resources.core),
        graphql: bucket(limits.resources.graphql),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Recorded 2026-10-10 with every limit to spare.
    const RECORDED: &str = include_str!("../../../../../fixtures/gh-api-rate-limit.json");

    #[test]
    fn the_core_and_graphql_limits_are_read_with_their_used_remaining_and_reset() {
        let limits = parse_rate_limit(RECORDED.as_bytes()).unwrap();

        let bucket = |used, remaining, reset| RateBucket {
            used,
            remaining,
            reset: Timestamp(reset),
        };
        assert_eq!(limits.core, bucket(0, 5000, 1_791_644_854));
        assert_eq!(limits.graphql, bucket(7, 4993, 1_791_644_766));
    }

    #[test]
    fn an_answer_with_no_resources_is_unreadable() {
        assert!(matches!(
            parse_rate_limit(br#"{"rate":{}}"#),
            Err(ForgeError::Unreadable(_))
        ));
    }
}
