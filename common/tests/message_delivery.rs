mod support;

use drone_common::wire::event::{Armed, Event, EventId, EventKind};
use drone_common::wire::message::*;
use drone_common::wire::reply::Outcome;
use support::*;

const ACK: u8 = 0x08;

fn ack(largest_received: u16, received_before: u32) -> Ack {
    return Ack {
        largest_received,
        ack_delay_us: 0,
        received_before,
    };
}

#[test]
fn ack_message() {
    let ack = Ack {
        largest_received: 0x0102,
        ack_delay_us: 20_000,
        received_before: 0x0605_0403,
    };
    let payload = [0x02, 0x01, 0x20, 0x4E, 0x03, 0x04, 0x05, 0x06];
    check_round_trip(Message::Ack(ack), ACK, &payload);
}

#[test]
fn ack_rejects_wrong_length() {
    check_only_length(ACK, &[0; 8]);
}

#[test]
fn ack_acknowledges_largest_received_and_the_bits_before() {
    // Bit n stands for Sequence largest_received − 1 − n (protocol.md §5.0a).
    let ack = ack(100, 0b101);
    let acknowledged: Vec<u16> = (0..=200)
        .filter(|&sequence| return ack.acknowledges(sequence))
        .collect();
    assert_eq!(acknowledged, [97, 99, 100]);
}

#[test]
fn ack_covers_33_sequences() {
    let all = ack(100, u32::MAX);
    assert!(all.acknowledges(100 - 32));
    assert!(!all.acknowledges(100 - 33));
    assert!(!all.acknowledges(101), "newer than largest_received");
}

#[test]
fn ack_counts_back_across_the_wrap() {
    let ack = ack(1, 0b11);
    assert!(ack.acknowledges(0));
    assert!(ack.acknowledges(u16::MAX));
    assert!(!ack.acknowledges(u16::MAX - 1));
}

#[test]
fn reliable_messages_are_requests_answers_and_events() {
    // protocol.md §5.0a: these are kept until acknowledged and resent when lost.
    let arm = CommandMessage::new(RequestId(1), 0, Command::Arm);
    let event = Event {
        event_id: EventId(1),
        kind: EventKind::Armed(Armed),
    };
    for message in [
        Message::Command(arm),
        Message::Reply(arm.reply(Outcome::Ok)),
        Message::Event(event),
    ] {
        assert!(message.is_reliable(), "{message:?}");
    }
}

#[test]
fn everything_else_may_be_lost() {
    let unreliable = [
        Message::Hello(Hello {
            protocol_version: 1,
            options: HelloOptions::default(),
        }),
        Message::HelloReply(HelloReply {
            protocol_version: 1,
            result: HelloResult::Ok,
            options: Options::default(),
        }),
        Message::Ping(Ping { timestamp: 0 }),
        Message::Pong(Pong {
            ping_time: 0,
            receive_time: 0,
            send_time: 0,
        }),
        Message::Ack(ack(0, 0)),
        Message::Sticks(Sticks {
            throttle: 0,
            roll: 0,
            pitch: 0,
            yaw: 0,
        }),
    ];
    for message in unreliable {
        assert!(!message.is_reliable(), "{message:?}");
    }
}

#[test]
fn sequences_before_counts_back_from_the_newest() {
    assert_eq!(sequences_before(100, 100), Some(0));
    assert_eq!(sequences_before(100, 97), Some(3));
    assert_eq!(sequences_before(1, u16::MAX), Some(2)); // across the wrap
    assert_eq!(sequences_before(100, 101), None); // newer
}

#[test]
fn sequences_before_splits_the_range_in_half() {
    // Less than half the range back is before; exactly half or more counts as newer.
    assert_eq!(sequences_before(0x7FFF, 0), Some(0x7FFF));
    assert_eq!(sequences_before(0x8000, 0), None);
}

#[test]
fn an_ack_reaches_32_back() {
    assert_eq!(ACK_REACH, 32);
    let ack = ack(100, u32::MAX);
    assert!(ack.acknowledges(100 - ACK_REACH));
    assert!(!ack.acknowledges(100 - ACK_REACH - 1));
}
