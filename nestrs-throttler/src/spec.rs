//! [`ThrottleSpec`] and [`ThrottleOutcome`]: the wire-shape of the throttle
//! contract. Specs parse from the decorator value emitted by `#[throttle]`;
//! outcomes are what the backend returns (allowed with remaining budget, or
//! limited with retry-after seconds).

/// Fixed request budget over a sliding-start window (`"5/minute"` ⇒ 5 per 60s).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThrottleSpec {
    pub limit: u64,
    pub window_secs: u64,
}

impl ThrottleSpec {
    /// Parse the decorator value emitted by `#[throttle(n, "per")]`.
    pub fn parse(s: &str) -> Option<Self> {
        let (limit, per) = s.split_once('/')?;
        let limit: u64 = limit.trim().parse().ok()?;
        let window_secs = match per.trim() {
            "second" => 1,
            "minute" => 60,
            "hour" => 3600,
            _ => return None,
        };
        if limit == 0 {
            return None;
        }
        Some(Self { limit, window_secs })
    }
}

/// Result of one throttle check against a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThrottleOutcome {
    Allowed {
        /// Requests still available in the current window (including this one).
        remaining: u64,
    },
    Limited {
        /// Seconds until the window resets (for `Retry-After`).
        retry_after_secs: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn throttle_spec_parse_valid_and_invalid() {
        assert_eq!(
            ThrottleSpec::parse("5/minute"),
            Some(ThrottleSpec {
                limit: 5,
                window_secs: 60
            })
        );
        assert_eq!(
            ThrottleSpec::parse("10/second"),
            Some(ThrottleSpec {
                limit: 10,
                window_secs: 1
            })
        );
        assert_eq!(
            ThrottleSpec::parse("2/hour"),
            Some(ThrottleSpec {
                limit: 2,
                window_secs: 3600
            })
        );
        assert_eq!(ThrottleSpec::parse("5/day"), None);
        assert_eq!(ThrottleSpec::parse("minute"), None);
        assert_eq!(ThrottleSpec::parse("0/minute"), None);
        assert_eq!(ThrottleSpec::parse("x/minute"), None);
    }
}
