//! One frame (protocol.md §4: envelope, CRC-16, COBS) through `Message::encode` and `Message::decode`.
mod support;

use drone_common::wire::message::{DecodeError, Message, Ping, Pong, Received};
use support::*;

const PING: Message = Message::Ping(Ping { timestamp: 0x11223344 });

#[test]
fn ping_frame_matches_bytes_computed_independently() {
    // Computed with a separate Python CRC-16/CCITT-FALSE (checked against "123456789" → 0x29B1) and COBS.
    assert_eq!(
        encode(&PING, 0x1234),
        [0x01, 0x0A, 0x03, 0x34, 0x12, 0x44, 0x33, 0x22, 0x11, 0x44, 0xDA, 0x00]
    );
    assert_eq!(
        encode(&Message::Ping(Ping { timestamp: 0 }), 0),
        [0x01, 0x02, 0x03, 0x01, 0x01, 0x01, 0x01, 0x01, 0x03, 0xBC, 0xE9, 0x00]
    );
}

#[test]
fn library_agrees_with_the_test_helpers() {
    assert_eq!(encode(&PING, 0x1234), frame(0, 0x03, 0x1234, &[0x44, 0x33, 0x22, 0x11]));
}

#[test]
fn pong_frame_is_20_bytes_with_only_a_trailing_zero() {
    // 6 envelope + 12 payload + 2 for COBS overhead and delimiter (protocol.md §4.2).
    let pong = Message::Pong(Pong {
        ping_time: 0,
        receive_time: 0,
        send_time: 0,
    });
    let bytes = encode(&pong, 0);
    assert_eq!(bytes.len(), 20);
    assert_eq!(bytes.last(), Some(&0));
    assert!(!bytes[..19].contains(&0));
}

#[test]
fn decode_returns_sequence_and_message() {
    assert_eq!(
        Message::decode(&encode(&PING, 5)),
        Ok(Received {
            sequence: 5,
            message: PING
        })
    );
}

#[test]
fn decode_rejects_a_frame_without_its_delimiter() {
    let bytes = encode(&PING, 5);
    assert_eq!(Message::decode(&bytes[..bytes.len() - 1]), Err(DecodeError::BadCobs));
}

#[test]
fn decode_rejects_empty_input() {
    assert_eq!(Message::decode(&[]), Err(DecodeError::BadCobs));
    assert_eq!(Message::decode(&[0x00]), Err(DecodeError::TooShort));
}

#[test]
fn decode_rejects_zeros_other_than_the_one_delimiter() {
    // Possible over UDP, where a packet goes to decode without the splitter.
    let bytes = encode(&PING, 5);
    assert_eq!(
        Message::decode(&[bytes.as_slice(), &[0]].concat()),
        Err(DecodeError::BadCobs)
    );
    let mut zero_inside = bytes.clone();
    zero_inside[4] = 0;
    assert_eq!(Message::decode(&zero_inside), Err(DecodeError::BadCobs));
}

#[test]
fn decode_never_panics_on_garbage() {
    // Release builds fly, so arbitrary bytes from the radio must only ever give an error.
    let mut state: u32 = 7;
    for len in 0..600 {
        let bytes: Vec<u8> = (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                return (state >> 16) as u8;
            })
            .collect();
        let _ = Message::decode(&bytes);
        let _ = Message::decode(&[bytes.as_slice(), &[0]].concat());
    }
}

#[test]
fn sequence_comes_back_unchanged() {
    for sequence in [1, 0x1234, 65535] {
        assert_eq!(
            Message::decode(&encode(&PING, sequence)).map(|r| return r.sequence),
            Ok(sequence)
        );
    }
}

#[test]
fn rejects_too_short() {
    assert_eq!(
        Message::decode(&with_crc_framed(vec![0x00, 0x03, 0x00])),
        Err(DecodeError::TooShort)
    );
}

#[test]
fn rejects_more_than_255_bytes() {
    let envelope: Vec<u8> = [0x00, 0xEE, 0x00, 0x00].into_iter().chain([1; 250]).collect();
    let bytes = with_crc_framed(envelope);
    assert_eq!(Message::decode(&bytes), Err(DecodeError::TooLong));
}

#[test]
fn accepts_exactly_255_bytes() {
    // An unknown type is only reported after length, COBS and CRC passed, so this shows a full envelope gets through.
    for payload in [[0u8; 249], [1u8; 249]] {
        let bytes = frame(0, 0xEE, 0, &payload);
        assert_eq!(
            Message::decode(&bytes),
            Err(DecodeError::UnknownType(0xEE)),
            "payload {}",
            payload[0]
        );
    }
}

/// A sequence number that leaves no zero after the flags byte of the envelope `flags, 0xEE, sequence, 249 × 0x01`.
fn sequence_without_zeros(flags: u8) -> u16 {
    return (0x0101..=u16::MAX)
        .find(|&sequence| {
            let mut envelope = vec![flags, 0xEE];
            envelope.extend_from_slice(&sequence.to_le_bytes());
            envelope.extend_from_slice(&[1; 249]);
            return !envelope[1..].contains(&0) && !crc16(&envelope).to_le_bytes().contains(&0);
        })
        .unwrap();
}

#[test]
fn decodes_a_254_byte_run_that_ends_the_frame() {
    // After the flags byte, all 254 bytes are non-zero: one full COBS block (code 0xFF) and nothing after it.
    let bytes = frame(0, 0xEE, sequence_without_zeros(0), &[1; 249]);
    assert_eq!(bytes[1], 0xFF);
    assert_eq!(Message::decode(&bytes), Err(DecodeError::UnknownType(0xEE)));
}

#[test]
fn decodes_a_254_byte_run_followed_by_more_data() {
    // Flags 0x01 makes all 255 bytes non-zero: a full block, then a second block with the last byte.
    // Flags are checked after COBS and CRC, so UnsupportedFlags shows the frame got that far.
    let bytes = frame(0x01, 0xEE, sequence_without_zeros(0x01), &[1; 249]);
    assert_eq!(bytes[0], 0xFF);
    assert_eq!(Message::decode(&bytes), Err(DecodeError::UnsupportedFlags));
}

#[test]
fn decodes_zero_heavy_payloads_of_every_length() {
    // Pseudo-random bytes with plenty of zeros, so zeros land everywhere in the COBS blocks.
    let mut state: u32 = 1;
    for len in 0..=249 {
        let payload: Vec<u8> = (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let b = (state >> 16) as u8;
                if b < 40 { return 0 } else { return b }
            })
            .collect();
        assert_eq!(
            Message::decode(&frame(0, 0xEE, 0, &payload)),
            Err(DecodeError::UnknownType(0xEE)),
            "payload of {len} bytes"
        );
    }
}

#[test]
fn rejects_a_flipped_bit_in_any_byte_as_bad_crc() {
    let mut envelope = vec![0x00, 0x03, 0x34, 0x12, 0x44, 0x33, 0x22, 0x11];
    let crc = crc16(&envelope);
    envelope.extend_from_slice(&crc.to_le_bytes());
    for index in 0..envelope.len() {
        let mut corrupted = envelope.clone();
        corrupted[index] ^= 0x01;
        assert_eq!(
            Message::decode(&cobs_frame(&corrupted)),
            Err(DecodeError::BadCrc),
            "byte {index} flipped"
        );
    }
}

#[test]
fn rejects_flags_until_authentication_exists() {
    let bytes = frame(0x01, 0x03, 0, &[0x44, 0x33, 0x22, 0x11]);
    assert_eq!(Message::decode(&bytes), Err(DecodeError::UnsupportedFlags));
}

#[test]
fn reports_unknown_type_with_its_byte() {
    // The drone answers with Reply "unknown type", so it must see the byte.
    assert_eq!(
        Message::decode(&frame(0, 0xEE, 1, &[])),
        Err(DecodeError::UnknownType(0xEE))
    );
}

#[test]
fn rejects_broken_cobs() {
    // Code byte 0x05 promises four data bytes, only two follow.
    assert_eq!(Message::decode(&[0x05, 0x11, 0x22, 0x00]), Err(DecodeError::BadCobs));
}
