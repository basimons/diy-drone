//! The round-trip time estimate and the probe timeout that follows from it (protocol.md §5.0a, as in RFC 9002).

use crate::ack::MAX_ACK_DELAY;
use crate::time::MAX_SPAN;
use core::time::Duration;

/// The round trip assumed before anything was measured (protocol.md §5.0a).
pub const INITIAL_RTT: Duration = Duration::from_millis(100);

/// The smallest variation the probe timeout allows for, so a very steady link doesn't get a hair-trigger timeout.
const GRANULARITY: Duration = Duration::from_millis(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RttEstimator {
    smoothed: Duration,
    variation: Duration,
    measured: bool, // false: `smoothed` is still the starting guess, which the first sample replaces
}

impl RttEstimator {
    /// Starts from a guess: the Ping/Pong round trip if there is one, else `INITIAL_RTT`.
    pub fn new(initial: Duration) -> Self {
        return Self {
            smoothed: initial,
            variation: initial / 2,
            measured: false,
        };
    }

    pub fn smoothed(&self) -> Duration {
        return self.smoothed;
    }

    pub fn variation(&self) -> Duration {
        return self.variation;
    }

    /// Adds one measurement: from sending a message to the Ack for it. `ack_delay` (from that Ack) is the time the
    /// other side held the Ack back, and is not part of the round trip.
    pub fn add_sample(&mut self, round_trip: Duration, ack_delay: Duration) {
        debug_assert!(
            round_trip < MAX_SPAN,
            "a round trip of {round_trip:?} is beyond the session clock"
        );
        // The other side promised to answer within MAX_ACK_DELAY; more is capped, as in RFC 9002.
        let sample = match round_trip.checked_sub(ack_delay.min(MAX_ACK_DELAY)) {
            Some(sample) if !sample.is_zero() => sample,
            _ => round_trip, // an Ack delay this long can't be right
        };
        if self.measured {
            self.variation = (self.variation * 3 + self.smoothed.abs_diff(sample)) / 4;
            self.smoothed = (self.smoothed * 7 + sample) / 8;
        } else {
            *self = Self {
                smoothed: sample,
                variation: sample / 2,
                measured: true,
            };
        }
    }

    /// How long to wait for an Ack before counting a message as lost: smoothed RTT + max(4 × RTT variation, 1 ms) +
    /// the longest the other side may hold an Ack back.
    pub fn probe_timeout(&self) -> Duration {
        return self.smoothed + (self.variation * 4).max(GRANULARITY) + MAX_ACK_DELAY;
    }
}

impl Default for RttEstimator {
    fn default() -> Self {
        return Self::new(INITIAL_RTT);
    }
}
