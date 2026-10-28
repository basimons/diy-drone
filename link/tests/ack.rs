use core::time::Duration;
use drone_common::wire::message::Ack;
use drone_link::ack::{AckTracker, Arrival, MAX_ACK_DELAY};
use drone_link::time::SessionTime;

const RELIABLE: bool = true;
const UNRELIABLE: bool = false;

fn at(ms: u32) -> SessionTime {
    return SessionTime(ms * 1_000);
}

/// A tracker that received `sequences`, all unreliable, at time 0.
fn received(sequences: &[u16]) -> AckTracker {
    let mut tracker = AckTracker::new();
    for &sequence in sequences {
        tracker.on_receive(sequence, UNRELIABLE, at(0));
    }
    return tracker;
}

#[test]
fn nothing_is_owed_before_anything_arrives() {
    let mut tracker = AckTracker::new();
    assert_eq!(tracker.ack_due(at(0)), None);
    assert_eq!(tracker.ack(at(0)), None);
}

#[test]
fn only_a_reliable_message_makes_an_ack_owed() {
    let mut tracker = AckTracker::new();
    tracker.on_receive(0, UNRELIABLE, at(0));
    assert_eq!(tracker.ack_due(at(0)), None);
    tracker.on_receive(1, RELIABLE, at(5));
    assert_eq!(tracker.ack_due(at(5)), Some(MAX_ACK_DELAY));
    assert_eq!(tracker.ack_due(at(15)), Some(Duration::from_millis(10)));
    assert_eq!(tracker.ack_due(at(40)), Some(Duration::ZERO)); // overdue
}

#[test]
fn the_first_reliable_message_waiting_sets_the_deadline() {
    let mut tracker = AckTracker::new();
    tracker.on_receive(0, RELIABLE, at(0));
    tracker.on_receive(1, RELIABLE, at(10));
    assert_eq!(tracker.ack_due(at(10)), Some(Duration::from_millis(10)));
}

#[test]
fn sending_the_ack_settles_what_was_owed() {
    let mut tracker = AckTracker::new();
    tracker.on_receive(0, RELIABLE, at(0));
    assert!(tracker.ack(at(1)).is_some());
    assert_eq!(tracker.ack_due(at(1)), None);
}

#[test]
fn an_ack_can_be_built_when_none_is_owed() {
    // Whether to send one anyway is the caller's choice.
    let mut tracker = received(&[3]);
    assert_eq!(tracker.ack(at(0)).map(|ack| return ack.largest_received), Some(3));
}

#[test]
fn ack_covers_the_newest_and_the_32_before_it() {
    let mut tracker = received(&[10, 11, 13, 14]);
    let ack = tracker.ack(at(0)).unwrap();
    assert_eq!(ack.largest_received, 14);
    assert_eq!(ack.received_before, 0b1101); // bit 0 = 13, bit 1 = 12 (missing), bit 2 = 11, bit 3 = 10
}

#[test]
fn ack_delay_is_the_time_since_the_newest_arrived() {
    let mut tracker = AckTracker::new();
    tracker.on_receive(0, RELIABLE, at(0));
    tracker.on_receive(1, UNRELIABLE, at(4));
    assert_eq!(tracker.ack(at(7)).unwrap().ack_delay_us, 3_000);
}

#[test]
fn ack_delay_too_long_for_the_field_is_capped() {
    let mut tracker = received(&[0]);
    assert_eq!(tracker.ack(at(70)).unwrap().ack_delay_us, u16::MAX);
}

#[test]
fn arrivals_say_how_each_message_fits_in() {
    let mut tracker = AckTracker::new();
    assert_eq!(tracker.on_receive(5, UNRELIABLE, at(0)), Arrival::Newest { skipped: 0 });
    assert_eq!(tracker.on_receive(6, UNRELIABLE, at(0)), Arrival::Newest { skipped: 0 });
    assert_eq!(tracker.on_receive(9, UNRELIABLE, at(0)), Arrival::Newest { skipped: 2 });
    assert_eq!(tracker.on_receive(7, UNRELIABLE, at(0)), Arrival::Late);
    assert_eq!(tracker.on_receive(7, UNRELIABLE, at(0)), Arrival::Duplicate);
    assert_eq!(tracker.on_receive(9, UNRELIABLE, at(0)), Arrival::Duplicate);
}

#[test]
fn a_message_beyond_what_an_ack_can_cover_is_too_old() {
    let mut tracker = received(&[100]);
    assert_eq!(tracker.on_receive(100 - 32, UNRELIABLE, at(0)), Arrival::Late);
    assert_eq!(tracker.on_receive(100 - 33, UNRELIABLE, at(0)), Arrival::TooOld);
}

#[test]
fn a_too_old_reliable_message_owes_no_ack() {
    // No Ack can cover it any more; the sender finds out it is lost from the probe timeout or a later Ack.
    let mut tracker = received(&[100]);
    tracker.on_receive(50, RELIABLE, at(0));
    assert_eq!(tracker.ack_due(at(0)), None);
}

#[test]
fn a_late_or_duplicate_reliable_message_is_still_acknowledged() {
    for sequence in [99, 100] {
        let mut tracker = received(&[98, 100]);
        tracker.on_receive(sequence, RELIABLE, at(0));
        assert!(tracker.ack_due(at(0)).is_some(), "{sequence}");
        assert!(tracker.ack(at(0)).unwrap().acknowledges(sequence));
    }
}

#[test]
fn a_jump_further_than_the_ack_reaches_forgets_the_older_ones() {
    let mut tracker = received(&[0, 1]);
    assert_eq!(
        tracker.on_receive(40, UNRELIABLE, at(0)),
        Arrival::Newest { skipped: 38 }
    );
    assert_eq!(tracker.ack(at(0)).unwrap().received_before, 0);
}

#[test]
fn a_jump_of_32_keeps_only_the_previous_newest() {
    let mut tracker = received(&[0, 1]);
    tracker.on_receive(33, UNRELIABLE, at(0));
    assert_eq!(tracker.ack(at(0)).unwrap().received_before, 1 << 31); // 1 still covered, 0 not
}

#[test]
fn sequences_wrap() {
    let mut tracker = received(&[u16::MAX - 1, u16::MAX]);
    assert_eq!(tracker.on_receive(1, UNRELIABLE, at(0)), Arrival::Newest { skipped: 1 });
    let ack: Ack = tracker.ack(at(0)).unwrap();
    for sequence in [u16::MAX - 1, u16::MAX, 1] {
        assert!(ack.acknowledges(sequence), "{sequence}");
    }
    assert!(!ack.acknowledges(0));
}

#[test]
fn a_jump_of_33_keeps_nothing() {
    let mut tracker = received(&[0, 1]);
    assert_eq!(
        tracker.on_receive(34, UNRELIABLE, at(0)),
        Arrival::Newest { skipped: 32 }
    );
    assert_eq!(tracker.ack(at(0)).unwrap().received_before, 0);
}

#[test]
fn a_late_arrival_32_back_is_the_last_bit() {
    let mut tracker = received(&[100]);
    tracker.on_receive(68, UNRELIABLE, at(0));
    let ack = tracker.ack(at(0)).unwrap();
    assert_eq!(ack.received_before, 1 << 31);
    assert!(ack.acknowledges(68));
}

#[test]
fn half_the_range_ahead_counts_as_newer() {
    // The same split as `sequences_before` in drone-common, which the Ack and the outbox use too.
    let mut tracker = received(&[0]);
    assert_eq!(
        tracker.on_receive(0x8000, UNRELIABLE, at(0)),
        Arrival::Newest { skipped: 0x7FFF }
    );
}
