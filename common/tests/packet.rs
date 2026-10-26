//! Packets (protocol.md §4.1: flags, type, sequence, payload, CRC-16): `Message::encode` and `Packet::parse`.
mod support;

use drone_common::wire::message::{Message, Ping, Pong};
use drone_common::wire::packet::{MAX_PACKET_LEN, Packet, PacketError};
use support::*;

const PING: Message = Message::Ping(Ping { timestamp: 0x11223344 });

#[test]
fn ping_packet_matches_bytes_computed_independently() {
    // Computed with a separate Python CRC-16/CCITT-FALSE, checked against "123456789" → 0x29B1.
    assert_eq!(
        encode_packet(&PING, 0x1234),
        [0x00, 0x03, 0x34, 0x12, 0x44, 0x33, 0x22, 0x11, 0x44, 0xDA]
    );
    assert_eq!(
        encode_packet(&Message::Ping(Ping { timestamp: 0 }), 0),
        [0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xBC, 0xE9]
    );
}

#[test]
fn library_agrees_with_the_test_helpers() {
    assert_eq!(
        encode_packet(&PING, 0x1234),
        packet(0, 0x03, 0x1234, &[0x44, 0x33, 0x22, 0x11])
    );
}

#[test]
fn pong_packet_is_18_bytes() {
    // 6 bytes of flags, type, sequence and CRC, plus 12 payload.
    let pong = Message::Pong(Pong {
        ping_time: 0,
        receive_time: 0,
        send_time: 0,
    });
    assert_eq!(encode_packet(&pong, 0).len(), 18);
}

#[test]
fn encoded_packet_parses_back() {
    let mut out = [0u8; MAX_PACKET_LEN];
    let encoded = PING.encode(0x1234, &mut out);
    let parsed = Packet::parse(encoded.as_bytes()).unwrap();
    assert_eq!(parsed, encoded);
    assert_eq!(parsed.sequence(), 0x1234);
    assert_eq!(Message::decode(&parsed), Ok(PING));
}

#[test]
fn sequence_comes_back_unchanged() {
    for sequence in [0, 1, 0x1234, 65535] {
        let bytes = encode_packet(&PING, sequence);
        assert_eq!(Packet::parse(&bytes).map(|p| return p.sequence()), Ok(sequence));
    }
}

#[test]
fn rejects_too_short() {
    assert_eq!(Packet::parse(&[]), Err(PacketError::TooShort));
    assert_eq!(
        Packet::parse(&with_crc(vec![0x00, 0x03, 0x00])),
        Err(PacketError::TooShort)
    );
}

#[test]
fn accepts_exactly_255_bytes_and_rejects_more() {
    let longest = packet(0, 0xEE, 0, &[1; 249]);
    assert_eq!(longest.len(), 255);
    assert!(Packet::parse(&longest).is_ok());
    let too_long = packet(0, 0xEE, 0, &[1; 250]);
    assert_eq!(Packet::parse(&too_long), Err(PacketError::TooLong));
}

#[test]
fn rejects_a_flipped_bit_in_any_byte_as_bad_crc() {
    let bytes = encode_packet(&PING, 0x1234);
    for index in 0..bytes.len() {
        for bit in 0..8 {
            let mut corrupted = bytes.clone();
            corrupted[index] ^= 1 << bit;
            assert_eq!(
                Packet::parse(&corrupted),
                Err(PacketError::BadCrc),
                "byte {index} bit {bit} flipped"
            );
        }
    }
}

#[test]
fn rejects_flags_until_authentication_exists() {
    let bytes = packet(0x01, 0x03, 0, &[0x44, 0x33, 0x22, 0x11]);
    assert_eq!(Packet::parse(&bytes), Err(PacketError::UnsupportedFlags));
}

#[test]
fn unknown_type_is_still_a_valid_packet() {
    // Intact but not understood: the drone answers it with a Reply, so it must get past parsing.
    assert!(Packet::parse(&packet(0, 0xEE, 1, &[])).is_ok());
}

#[test]
fn decode_never_panics_on_intact_packets_with_garbage_inside() {
    // Random bytes almost never pass the CRC, so give garbage a correct CRC to reach `Message::decode`.
    let mut state: u32 = 11;
    let mut next = || {
        state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        return (state >> 16) as u8;
    };
    for len in 0..2000 {
        let message_type = if len % 2 == 0 { (len / 2 % 6) as u8 } else { next() }; // often a known type
        let payload: Vec<u8> = (0..len % 250).map(|_| return next()).collect();
        let bytes = packet(0, message_type, u16::from(next()), &payload);
        let _ = Message::decode(&Packet::parse(&bytes).unwrap());
    }
}

#[test]
fn parse_never_panics_on_garbage() {
    // Release builds fly, so arbitrary bytes from the radio must only ever give an error.
    let mut state: u32 = 7;
    for len in 0..600 {
        let bytes: Vec<u8> = (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                return (state >> 16) as u8;
            })
            .collect();
        if let Ok(packet) = Packet::parse(&bytes) {
            let _ = Message::decode(&packet);
        }
    }
}
