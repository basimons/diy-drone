//! The receiving side of delivery (protocol.md §5.0a).

use crate::time::SessionTime;
use core::time::Duration;
use drone_common::wire::message::{ACK_REACH, Ack, sequences_before};

/// How long a side may hold an Ack back after a reliable message arrived (protocol.md §5.0a).
pub const MAX_ACK_DELAY: Duration = Duration::from_millis(20);

/// How an arriving Sequence fits in with the ones before it. For counting gaps and spotting copies; acknowledging is
/// handled either way.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Arrival {
    Newest { skipped: u16 }, // newer than all before; `skipped` Sequences in between haven't arrived (yet)
    Late,                    // older than the newest, fills a gap
    Duplicate,               // arrived before (UDP can deliver a datagram twice)
    TooOld,                  // more than `ACK_REACH` before the newest: no Ack can cover it, so it can't be told apart
}

/// Which of the other side's Sequences arrived, and when an Ack is owed (protocol.md §5.0a). One per session, fed from
/// its first message (the Hello or HelloReply, Sequence 0), so `skipped` counts every gap.
#[derive(Clone, Copy, Debug, Default)]
pub struct AckTracker {
    newest: Option<Newest>,
    owed_since: Option<SessionTime>, // when the oldest reliable message not yet acknowledged arrived
}

#[derive(Clone, Copy, Debug)]
struct Newest {
    sequence: u16,
    arrived_at: SessionTime,
    received_before: u32, // as in the Ack: bit n set means `sequence` − 1 − n arrived
}

impl AckTracker {
    pub fn new() -> Self {
        return Self::default();
    }

    /// Records an arrived message. `reliable` (`Message::is_reliable`): it must be acknowledged within
    /// `MAX_ACK_DELAY`.
    pub fn on_receive(&mut self, sequence: u16, reliable: bool, now: SessionTime) -> Arrival {
        let arrival = match &mut self.newest {
            None => {
                self.newest = Some(Newest {
                    sequence,
                    arrived_at: now,
                    received_before: 0,
                });
                Arrival::Newest { skipped: 0 }
            }
            Some(newest) => newest.add(sequence, now),
        };
        if reliable && arrival != Arrival::TooOld && self.owed_since.is_none() {
            self.owed_since = Some(now);
        }
        return arrival;
    }

    /// `None` if no Ack is owed; else how long until it must be sent, zero once that time has passed.
    pub fn ack_due(&self, now: SessionTime) -> Option<Duration> {
        return self
            .owed_since
            .map(|since| return MAX_ACK_DELAY.saturating_sub(now.since(since)));
    }

    /// The Ack for everything received so far, which settles what was owed. `None` before anything arrived. Can be
    /// built when none is owed; protocol.md §5.0a only sends one when one is.
    pub fn ack(&mut self, now: SessionTime) -> Option<Ack> {
        let newest = self.newest?;
        self.owed_since = None;
        let delay_us = now.since(newest.arrived_at).as_micros();
        return Some(Ack {
            largest_received: newest.sequence,
            ack_delay_us: u16::try_from(delay_us).unwrap_or(u16::MAX),
            received_before: newest.received_before,
        });
    }
}

impl Newest {
    fn add(&mut self, sequence: u16, now: SessionTime) -> Arrival {
        return match sequences_before(self.sequence, sequence) {
            Some(0) => Arrival::Duplicate,
            Some(behind @ 1..=ACK_REACH) => {
                let bit = 1 << (behind - 1);
                let seen = self.received_before & bit != 0;
                self.received_before |= bit;
                if seen { Arrival::Duplicate } else { Arrival::Late }
            }
            Some(_) => Arrival::TooOld,
            None => {
                let ahead = sequence.wrapping_sub(self.sequence);
                // Shifting by `ahead` moves every bit along; the old newest becomes bit `ahead` − 1.
                let kept = self.received_before.checked_shl(u32::from(ahead)).unwrap_or(0);
                let previous = 1u32.checked_shl(u32::from(ahead - 1)).unwrap_or(0);
                *self = Newest {
                    sequence,
                    arrived_at: now,
                    received_before: kept | previous,
                };
                Arrival::Newest { skipped: ahead - 1 }
            }
        };
    }
}
