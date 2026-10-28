use core::time::Duration;
use drone_common::wire::message::Ack;
use drone_link::outbox::{Delivery, Full, Outbox};
use drone_link::rtt::INITIAL_RTT;
use drone_link::rtt::RttEstimator;
use drone_link::time::SessionTime;

/// The probe timeout before anything was measured: 100 ms + 4 × 50 ms + 20 ms.
const FIRST_PROBE_TIMEOUT_MS: u32 = 320;

fn at(ms: u32) -> SessionTime {
    return SessionTime(ms * 1_000);
}

fn ack(largest_received: u16, received_before: u32) -> Ack {
    return Ack {
        largest_received,
        ack_delay_us: 0,
        received_before,
    };
}

/// An outbox holding `items`, each sent at time 0 with the Sequence it is paired with.
fn sent<const N: usize>(items: &[(u16, &'static str)]) -> Outbox<&'static str, N> {
    let mut outbox = Outbox::new(RttEstimator::default());
    for &(sequence, item) in items {
        outbox.send(sequence, item, at(0)).unwrap();
    }
    return outbox;
}

fn on_ack<const N: usize>(
    outbox: &mut Outbox<&'static str, N>,
    ack: Ack,
    now: SessionTime,
) -> Vec<Delivery<&'static str>> {
    let mut deliveries = Vec::new();
    outbox.on_ack(&ack, now, |delivery| return deliveries.push(delivery));
    return deliveries;
}

fn timeouts<const N: usize>(outbox: &mut Outbox<&'static str, N>, now: SessionTime) -> Vec<Delivery<&'static str>> {
    let mut deliveries = Vec::new();
    outbox.check_timeouts(now, |delivery| return deliveries.push(delivery));
    return deliveries;
}

#[test]
fn an_acknowledged_message_is_reported_and_forgotten() {
    let mut outbox = sent::<4>(&[(5, "arm")]);
    assert_eq!(on_ack(&mut outbox, ack(5, 0), at(10)), [Delivery::Acknowledged("arm")]);
    assert!(outbox.is_empty());
}

#[test]
fn one_ack_can_acknowledge_several() {
    let mut outbox = sent::<4>(&[(5, "a"), (6, "b"), (8, "c")]);
    let deliveries = on_ack(&mut outbox, ack(8, 0b110), at(10)); // 8, then bit 1 = 6 and bit 2 = 5
    assert_eq!(
        deliveries,
        [
            Delivery::Acknowledged("a"),
            Delivery::Acknowledged("b"),
            Delivery::Acknowledged("c")
        ]
    );
}

#[test]
fn a_message_three_sequences_behind_an_acknowledged_one_is_lost() {
    let mut outbox = sent::<4>(&[(5, "a"), (6, "b")]);
    // 7 and 8 were unreliable and arrived; 5 and 6 did not.
    assert_eq!(on_ack(&mut outbox, ack(8, 0b1), at(10)), [Delivery::Lost("a")]);
    assert_eq!(outbox.len(), 1); // 6 is only 2 behind: it may still arrive (UDP reorders)
    assert_eq!(on_ack(&mut outbox, ack(9, 0b11), at(10)), [Delivery::Lost("b")]);
}

#[test]
fn a_message_older_than_the_ack_reaches_is_lost() {
    let mut outbox = sent::<4>(&[(0, "old")]);
    assert_eq!(on_ack(&mut outbox, ack(100, u32::MAX), at(10)), [Delivery::Lost("old")]);
}

#[test]
fn deliveries_are_reported_in_send_order() {
    let mut outbox = sent::<8>(&[(1, "a"), (2, "b"), (3, "c"), (4, "d")]);
    // 2 and 4 arrived, 1 is lost, 3 still may arrive.
    let deliveries = on_ack(&mut outbox, ack(4, 0b10), at(10));
    assert_eq!(
        deliveries,
        [
            Delivery::Lost("a"),
            Delivery::Acknowledged("b"),
            Delivery::Acknowledged("d")
        ]
    );
    assert_eq!(outbox.len(), 1);
}

#[test]
fn an_ack_for_the_newest_pending_message_measures_the_round_trip() {
    let mut outbox = sent::<4>(&[(5, "a")]);
    let ack = Ack {
        ack_delay_us: 5_000,
        ..ack(5, 0)
    };
    on_ack(&mut outbox, ack, at(35));
    assert_eq!(outbox.rtt().smoothed(), Duration::from_millis(30));
}

#[test]
fn no_round_trip_is_measured_when_largest_received_was_not_pending() {
    // Largest Received is an unreliable message (or acknowledged before): its send time isn't known here.
    let mut outbox = sent::<4>(&[(5, "a")]);
    on_ack(&mut outbox, ack(6, 0b1), at(35));
    assert_eq!(outbox.rtt().smoothed(), RttEstimator::default().smoothed());
}

#[test]
fn a_message_without_an_ack_within_the_probe_timeout_is_lost() {
    let mut outbox = sent::<4>(&[(5, "a")]);
    assert_eq!(timeouts(&mut outbox, at(FIRST_PROBE_TIMEOUT_MS - 1)), []);
    assert_eq!(timeouts(&mut outbox, at(FIRST_PROBE_TIMEOUT_MS)), [Delivery::Lost("a")]);
    assert!(outbox.is_empty());
}

#[test]
fn next_timeout_says_when_to_check_again() {
    let mut outbox = Outbox::<&str, 4>::new(RttEstimator::default());
    assert_eq!(outbox.next_timeout(at(0)), None);
    outbox.send(1, "a", at(0)).unwrap();
    outbox.send(2, "b", at(100)).unwrap();
    assert_eq!(outbox.next_timeout(at(120)), Some(Duration::from_millis(200))); // "a", the oldest
    assert_eq!(outbox.next_timeout(at(400)), Some(Duration::ZERO));
}

#[test]
fn each_probe_timeout_in_a_row_doubles_it() {
    let mut outbox = sent::<4>(&[(1, "a")]);
    let first = at(FIRST_PROBE_TIMEOUT_MS);
    assert_eq!(timeouts(&mut outbox, first), [Delivery::Lost("a")]);
    outbox.send(2, "a", first).unwrap(); // resent: new Sequence
    assert_eq!(outbox.next_timeout(first), Some(Duration::from_millis(2 * 320)));
}

#[test]
fn an_ack_resets_the_doubling() {
    let mut outbox = sent::<4>(&[(1, "a")]);
    let first = at(FIRST_PROBE_TIMEOUT_MS);
    timeouts(&mut outbox, first);
    outbox.send(2, "a", first).unwrap();
    on_ack(&mut outbox, ack(3, 0), first); // acknowledges an unreliable message only
    assert_eq!(outbox.next_timeout(first), Some(Duration::from_millis(320)));
}

#[test]
fn a_full_outbox_gives_the_item_back() {
    let mut outbox = sent::<2>(&[(1, "a"), (2, "b")]);
    assert_eq!(outbox.send(3, "c", at(0)), Err(Full("c")));
    assert_eq!(outbox.len(), 2);
}

#[test]
fn acknowledge_takes_out_a_message_known_to_have_arrived() {
    // The answer to a request arrived, which acknowledges it too (protocol.md §5.0a).
    let mut outbox = sent::<4>(&[(1, "a"), (2, "b"), (3, "c")]);
    assert_eq!(outbox.acknowledge(|&item| return item == "b"), Some("b"));
    assert_eq!(outbox.acknowledge(|&item| return item == "b"), None);
    assert_eq!(
        on_ack(&mut outbox, ack(3, 0b11), at(10)),
        [Delivery::Acknowledged("a"), Delivery::Acknowledged("c")]
    );
}

#[test]
fn sequences_wrap() {
    let mut outbox = sent::<4>(&[(u16::MAX, "a"), (0, "b")]);
    assert_eq!(on_ack(&mut outbox, ack(2, 0b1), at(10)), [Delivery::Lost("a")]);
    assert_eq!(on_ack(&mut outbox, ack(1, 0b1), at(10)), [Delivery::Acknowledged("b")]);
}

#[test]
fn holds_any_item_type() {
    // E.g. the whole message, to resend it as it was.
    let mut outbox = Outbox::<drone_common::wire::message::Message, 16>::new(RttEstimator::default());
    let arm = drone_common::wire::message::CommandMessage::new(
        drone_common::wire::message::RequestId(1),
        0,
        drone_common::wire::message::Command::Arm,
    );
    assert!(
        outbox
            .send(0, drone_common::wire::message::Message::Command(arm), at(0))
            .is_ok()
    );
}

#[test]
fn messages_lost_in_one_outage_double_the_timeout_once() {
    // "a" and "b" are sent 1 ms apart and both time out: one silence, not two timeouts in a row.
    let mut outbox = sent::<4>(&[(1, "a")]);
    outbox.send(2, "b", at(1)).unwrap();
    assert_eq!(timeouts(&mut outbox, at(320)), [Delivery::Lost("a")]);
    outbox.send(3, "a", at(320)).unwrap();
    assert_eq!(timeouts(&mut outbox, at(641)), [Delivery::Lost("b")]);
    outbox.send(4, "b", at(641)).unwrap();
    assert_eq!(
        outbox.next_timeout(at(641)),
        Some(Duration::from_millis(320 + 640 - 641))
    );
}

#[test]
fn a_resend_that_times_out_again_doubles_again() {
    let mut outbox = sent::<4>(&[(1, "a")]);
    timeouts(&mut outbox, at(320));
    outbox.send(2, "a", at(320)).unwrap();
    assert_eq!(timeouts(&mut outbox, at(320 + 640)), [Delivery::Lost("a")]);
    outbox.send(3, "a", at(960)).unwrap();
    assert_eq!(outbox.next_timeout(at(960)), Some(Duration::from_millis(4 * 320)));
}

#[test]
fn a_check_that_finds_nothing_does_not_double() {
    let mut outbox = sent::<4>(&[(1, "a")]);
    for ms in [10, 100, 200] {
        assert_eq!(timeouts(&mut outbox, at(ms)), []);
    }
    assert_eq!(outbox.next_timeout(at(200)), Some(Duration::from_millis(120)));
}

#[test]
fn doubling_stops_at_1024_times() {
    // Well inside the ~35 minutes the session clock can compare (protocol.md §5.1).
    let mut outbox = Outbox::<&str, 4>::new(RttEstimator::default());
    let mut now = at(0);
    for sequence in 0..20 {
        outbox.send(sequence, "a", now).unwrap();
        let wait = outbox.next_timeout(now).unwrap();
        now = SessionTime(now.0.wrapping_add(u32::try_from(wait.as_micros()).unwrap()));
        assert_eq!(timeouts(&mut outbox, now), [Delivery::Lost("a")]);
    }
    outbox.send(20, "a", now).unwrap();
    assert_eq!(outbox.next_timeout(now), Some(Duration::from_millis(320 * 1024)));
}

#[test]
fn an_answer_resets_the_doubling_like_an_ack() {
    let mut outbox = sent::<4>(&[(1, "a")]);
    timeouts(&mut outbox, at(320));
    outbox.send(2, "a", at(320)).unwrap();
    outbox.send(3, "b", at(320)).unwrap();
    outbox.acknowledge(|&item| return item == "a");
    assert_eq!(outbox.next_timeout(at(320)), Some(Duration::from_millis(320)));
}

#[test]
fn is_full_says_when_send_would_fail() {
    let mut outbox = sent::<2>(&[(1, "a")]);
    assert!(!outbox.is_full());
    outbox.send(2, "b", at(0)).unwrap();
    assert!(outbox.is_full());
}

#[test]
fn no_round_trip_is_measured_for_a_message_that_already_timed_out() {
    let mut outbox = sent::<4>(&[(1, "a")]);
    timeouts(&mut outbox, at(320));
    on_ack(&mut outbox, ack(1, 0), at(400)); // a late Ack for the first copy: no longer in the outbox
    assert_eq!(outbox.rtt().smoothed(), INITIAL_RTT);
}

#[test]
fn discard_takes_out_the_oldest_match() {
    // E.g. a full outbox on the drone makes room by dropping its oldest Event.
    let mut outbox = sent::<4>(&[(1, "event 1"), (2, "reply"), (3, "event 2")]);
    assert_eq!(outbox.discard(|item| return item.starts_with("event")), Some("event 1"));
    assert_eq!(outbox.discard(|item| return item.starts_with("nothing")), None);
    assert_eq!(
        on_ack(&mut outbox, ack(3, 0b1), at(10)),
        [Delivery::Acknowledged("reply"), Delivery::Acknowledged("event 2")]
    );
}

#[test]
fn discard_does_not_end_the_doubling() {
    // Unlike `acknowledge`: nothing arrived, so the link is no better than before.
    let mut outbox = sent::<4>(&[(1, "a")]);
    timeouts(&mut outbox, at(320));
    outbox.send(2, "a", at(320)).unwrap();
    outbox.send(3, "b", at(320)).unwrap();
    outbox.discard(|&item| return item == "b");
    assert_eq!(outbox.next_timeout(at(320)), Some(Duration::from_millis(2 * 320)));
}
