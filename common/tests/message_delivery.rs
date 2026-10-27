mod support;

use drone_common::wire::message::*;
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
