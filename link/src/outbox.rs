//! The sending side of delivery (protocol.md §5.0a).

use crate::rtt::RttEstimator;
use crate::time::{MAX_SPAN, SessionTime};
use core::time::Duration;
use drone_common::wire::message::{Ack, sequences_before};

/// What became of a message in the outbox. Either way it is gone from the outbox; a lost one is resent by sending it
/// again with a new Sequence, if the caller still wants it to arrive.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Delivery<T> {
    Acknowledged(T),
    Lost(T),
}

/// The outbox already holds as many messages as it can; the item that didn't fit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Full<T>(pub T);

/// Reliable messages sent but not yet acknowledged, up to `N`, each with whatever the caller wants back when it is
/// acknowledged or lost (`T`: the message itself, or an index into the caller's own list). One per session.
#[derive(Clone, Copy, Debug)]
pub struct Outbox<T, const N: usize> {
    pending: [Option<Pending<T>>; N], // in send order, all `Some`s before the first `None`
    rtt: RttEstimator,
    backoff: Option<Backoff>, // `None`: no probe timeout since the last Ack
}

#[derive(Clone, Copy, Debug)]
struct Pending<T> {
    sequence: u16,
    sent_at: SessionTime,
    item: T,
}

/// Probe timeouts in a row: each one that a resend runs into as well doubles the probe timeout.
#[derive(Clone, Copy, Debug)]
struct Backoff {
    doublings: u32,
    last_timeout: SessionTime,
}

enum Fate {
    Acknowledged,
    Lost,
}

/// A message this many Sequences older than one that arrived counts as lost; fewer leaves room for UDP reordering.
const LOSS_DISTANCE: u16 = 3;

/// Doubling stops at ×1024 (~5 minutes from the starting 320 ms), long after the link would have been declared lost,
/// and well inside `MAX_SPAN`.
const MAX_DOUBLINGS: u32 = 10;

impl<T, const N: usize> Outbox<T, N> {
    /// `rtt`: the starting estimate, `RttEstimator::default()` or one from a Ping/Pong.
    pub fn new(rtt: RttEstimator) -> Self {
        const { assert!(N > 0, "an Outbox must hold at least one message") };
        return Self {
            pending: [const { None }; N],
            rtt,
            backoff: None,
        };
    }

    /// Keeps `item` until the message sent with `sequence` is acknowledged or lost. Sequences must count up, as
    /// sending does.
    pub fn send(&mut self, sequence: u16, item: T, now: SessionTime) -> Result<(), Full<T>> {
        debug_assert!(
            self.iter()
                .all(|pending| return sequences_before(sequence, pending.sequence).is_some_and(|d| return d > 0)),
            "Sequence {sequence} isn't newer than every message in the outbox: every Sequence is sent once, in order"
        );
        self.debug_assert_not_before_sends(now);
        let Some(free) = self.pending.iter_mut().find(|slot| return slot.is_none()) else {
            return Err(Full(item));
        };
        *free = Some(Pending {
            sequence,
            sent_at: now,
            item,
        });
        return Ok(());
    }

    /// Reports, in send order, every message `ack` acknowledges and every one it shows to be lost. Measures the
    /// round trip when Largest Received is a message in the outbox.
    pub fn on_ack(&mut self, ack: &Ack, now: SessionTime, report: impl FnMut(Delivery<T>)) {
        self.debug_assert_not_before_sends(now);
        let newest = self
            .iter()
            .find(|pending| return pending.sequence == ack.largest_received);
        if let Some(newest) = newest {
            let ack_delay = Duration::from_micros(u64::from(ack.ack_delay_us));
            self.rtt.add_sample(now.since(newest.sent_at), ack_delay);
        }
        self.backoff = None;
        self.settle(
            |pending| {
                if ack.acknowledges(pending.sequence) {
                    return Some(Fate::Acknowledged);
                }
                return match sequences_before(ack.largest_received, pending.sequence) {
                    Some(behind) if behind >= LOSS_DISTANCE => Some(Fate::Lost),
                    _ => None,
                };
            },
            report,
        );
    }

    /// Reports as lost, in send order, every message without an Ack for longer than the probe timeout. The probe
    /// timeout doubles when a message sent after the previous timeout (a resend) times out too, until an Ack or an
    /// answer arrives. Call it at `next_timeout`; earlier does no harm.
    pub fn check_timeouts(&mut self, now: SessionTime, report: impl FnMut(Delivery<T>)) {
        self.debug_assert_not_before_sends(now);
        let timeout = self.probe_timeout();
        let last_timeout = self.backoff.map(|backoff| return backoff.last_timeout);
        let mut in_a_row = false;
        self.settle(
            |pending| {
                let waited = now.since(pending.sent_at);
                let lost = waited >= timeout;
                in_a_row |= lost && last_timeout.is_none_or(|last| return waited <= now.since(last));
                return lost.then_some(Fate::Lost);
            },
            report,
        );
        if in_a_row {
            let doublings = self.backoff.map_or(0, |backoff| return backoff.doublings);
            self.backoff = Some(Backoff {
                doublings: (doublings + 1).min(MAX_DOUBLINGS),
                last_timeout: now,
            });
        }
    }

    /// How long until `check_timeouts` has something to report; zero if it has now, `None` if the outbox is empty.
    pub fn next_timeout(&self, now: SessionTime) -> Option<Duration> {
        self.debug_assert_not_before_sends(now);
        let timeout = self.probe_timeout();
        return self
            .iter()
            .map(|pending| return timeout.saturating_sub(now.since(pending.sent_at)))
            .min();
    }

    /// Takes out the first item `matches` picks: a request whose answer arrived, which acknowledges it too
    /// (protocol.md §5.0a). Like an Ack, that ends the doubling of the probe timeout.
    pub fn acknowledge(&mut self, matches: impl FnMut(&T) -> bool) -> Option<T> {
        let acknowledged = self.take_first(matches)?;
        self.backoff = None;
        return Some(acknowledged);
    }

    /// Takes out the oldest item `matches` picks, which then is never resent: making room in a full outbox (on the
    /// drone: its oldest Event, else its oldest Reply; protocol.md §5.0a). Unlike `acknowledge`, nothing arrived, so
    /// the probe timeout stays as it is.
    pub fn discard(&mut self, matches: impl FnMut(&T) -> bool) -> Option<T> {
        return self.take_first(matches);
    }

    pub fn rtt(&self) -> &RttEstimator {
        return &self.rtt;
    }

    pub fn len(&self) -> usize {
        return self.iter().count();
    }

    pub fn is_empty(&self) -> bool {
        return self.pending[0].is_none();
    }

    /// Whether `send` would give the item back.
    pub fn is_full(&self) -> bool {
        return self.pending[N - 1].is_some();
    }

    fn probe_timeout(&self) -> Duration {
        let doublings = self.backoff.map_or(0, |backoff| return backoff.doublings);
        return self.rtt.probe_timeout().saturating_mul(1 << doublings);
    }

    fn take_first(&mut self, mut matches: impl FnMut(&T) -> bool) -> Option<T> {
        let index = self.iter().position(|pending| return matches(&pending.item))?;
        let taken = self.pending[index].take();
        self.pending[index..].rotate_left(1); // the rest move up, keeping their order
        return taken.map(|pending| return pending.item);
    }

    fn iter(&self) -> impl Iterator<Item = &Pending<T>> {
        return self.pending.iter().map_while(Option::as_ref);
    }

    /// Takes out every message `verdict` gives a fate, and reports it, in send order. The rest move up, keeping their
    /// order.
    fn settle(&mut self, mut verdict: impl FnMut(&Pending<T>) -> Option<Fate>, mut report: impl FnMut(Delivery<T>)) {
        let mut kept = 0;
        for index in 0..N {
            let Some(pending) = self.pending[index].take() else {
                break; // the `Some`s come first
            };
            match verdict(&pending) {
                Some(Fate::Acknowledged) => report(Delivery::Acknowledged(pending.item)),
                Some(Fate::Lost) => report(Delivery::Lost(pending.item)),
                None => {
                    self.pending[kept] = Some(pending);
                    kept += 1;
                }
            }
        }
        debug_assert!(self.pending[kept..].iter().all(Option::is_none), "a gap in the outbox");
    }

    /// A `now` before a send would wrap to ~71 minutes and make that message lost at once.
    fn debug_assert_not_before_sends(&self, now: SessionTime) {
        debug_assert!(
            self.iter().all(|pending| return now.since(pending.sent_at) < MAX_SPAN),
            "now ({now:?}) is before a message in the outbox was sent"
        );
    }
}
