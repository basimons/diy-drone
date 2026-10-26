//! Framing (protocol.md §4): envelope, CRC-16 and COBS, through `Message::encode_frame` and `FrameReader`.
mod support;

use drone_common::wire::frame::{FrameError, FrameReader, MAX_FRAME_LEN, Push, Received};
use drone_common::wire::message::{Message, Ping, Pong};
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
fn every_byte_before_the_delimiter_needs_more() {
    let bytes = encode(&PING, 5);
    let mut reader = FrameReader::new();
    for &byte in &bytes[..bytes.len() - 1] {
        assert_eq!(reader.push(byte), Push::NeedMore);
    }
    assert_eq!(
        reader.push(0),
        Push::Frame(Received {
            sequence: 5,
            message: PING
        })
    );
}

#[test]
fn sequence_comes_back_unchanged() {
    for sequence in [1, 0x1234, 65535] {
        assert_eq!(
            read_one(&encode(&PING, sequence)).map(|r| return r.sequence),
            Ok(sequence)
        );
    }
}

#[test]
fn rejects_too_short() {
    assert_eq!(
        read_one(&with_crc_framed(vec![0x00, 0x03, 0x00])),
        Err(FrameError::TooShort)
    );
}

#[test]
fn rejects_more_than_255_bytes() {
    let envelope: Vec<u8> = [0x00, 0xEE, 0x00, 0x00].into_iter().chain([1; 250]).collect();
    let bytes = with_crc_framed(envelope);
    assert_eq!(read_one(&bytes), Err(FrameError::TooLong));
}

#[test]
fn accepts_exactly_255_bytes() {
    // An unknown type is only reported after length, COBS and CRC passed, so this shows a full envelope gets through.
    for payload in [[0u8; 249], [1u8; 249]] {
        let bytes = frame(0, 0xEE, 0, &payload);
        assert_eq!(
            read_one(&bytes),
            Err(FrameError::UnknownType(0xEE)),
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
    assert_eq!(read_one(&bytes), Err(FrameError::UnknownType(0xEE)));
}

#[test]
fn decodes_a_254_byte_run_followed_by_more_data() {
    // Flags 0x01 makes all 255 bytes non-zero: a full block, then a second block with the last byte.
    // Flags are checked after COBS and CRC, so UnsupportedFlags shows the frame got that far.
    let bytes = frame(0x01, 0xEE, sequence_without_zeros(0x01), &[1; 249]);
    assert_eq!(bytes[0], 0xFF);
    assert_eq!(read_one(&bytes), Err(FrameError::UnsupportedFlags));
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
            read_one(&frame(0, 0xEE, 0, &payload)),
            Err(FrameError::UnknownType(0xEE)),
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
            read_one(&cobs_frame(&corrupted)),
            Err(FrameError::BadCrc),
            "byte {index} flipped"
        );
    }
}

#[test]
fn rejects_flags_until_authentication_exists() {
    let bytes = frame(0x01, 0x03, 0, &[0x44, 0x33, 0x22, 0x11]);
    assert_eq!(read_one(&bytes), Err(FrameError::UnsupportedFlags));
}

#[test]
fn reports_unknown_type_with_its_byte() {
    // The drone answers with Reply "unknown type", so it must see the byte.
    assert_eq!(read_one(&frame(0, 0xEE, 1, &[])), Err(FrameError::UnknownType(0xEE)));
}

#[test]
fn rejects_broken_cobs() {
    // Code byte 0x05 promises four data bytes, only two follow.
    assert_eq!(read_one(&[0x05, 0x11, 0x22, 0x00]), Err(FrameError::BadCobs));
}

#[test]
fn reader_returns_frames_sent_back_to_back() {
    let bytes = [encode(&PING, 1), encode(&PING, 2)].concat();
    assert_eq!(
        read_all(&bytes),
        vec![
            Ok(Received {
                sequence: 1,
                message: PING
            }),
            Ok(Received {
                sequence: 2,
                message: PING
            })
        ]
    );
}

#[test]
fn reader_ignores_empty_frames() {
    let bytes = [vec![0, 0, 0], encode(&PING, 1), vec![0]].concat();
    assert_eq!(read_all(&bytes).len(), 1);
}

#[test]
fn reader_recovers_after_garbage() {
    // Joining mid-frame: the partial frame fails, the next one is fine.
    let full = encode(&PING, 1);
    let results = read_all(&[full[3..].to_vec(), full.clone()].concat());
    assert_eq!(results.len(), 2);
    assert!(results[0].is_err());
    assert!(results[1].is_ok());
}

#[test]
fn reader_reports_corruption_and_carries_on() {
    let mut corrupted = encode(&PING, 0x1234);
    corrupted[5] ^= 0x40; // a data byte, not a COBS code byte
    let results = read_all(&[corrupted, encode(&PING, 1)].concat());
    assert_eq!(results.len(), 2);
    assert_eq!(results[0], Err(FrameError::BadCrc));
    assert!(results[1].is_ok());
}

#[test]
fn reader_rejects_overlong_frame_and_carries_on() {
    let bytes = [vec![0x55; MAX_FRAME_LEN + 10], vec![0], encode(&PING, 1)].concat();
    let results = read_all(&bytes);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0], Err(FrameError::TooLong));
    assert!(results[1].is_ok());
}

#[test]
fn reader_overflows_only_past_the_largest_possible_frame() {
    let longest = [vec![0x55; MAX_FRAME_LEN - 1], vec![0]].concat();
    let too_long = [vec![0x55; MAX_FRAME_LEN], vec![0]].concat();
    let results = read_all(&[longest, too_long, encode(&PING, 1)].concat());
    assert_eq!(results.len(), 3);
    assert!(
        matches!(results[0], Err(e) if e != FrameError::TooLong),
        "{:?}",
        results[0]
    );
    assert_eq!(results[1], Err(FrameError::TooLong));
    assert!(results[2].is_ok());
}

#[test]
fn reader_rejects_frame_that_decodes_to_more_than_255_bytes() {
    // Each 0x01 block stands for one zero: 257 of them decode to 256 zeros.
    assert_eq!(
        read_all(&[vec![0x01; MAX_FRAME_LEN - 1], vec![0]].concat()),
        vec![Err(FrameError::TooLong)]
    );
}

#[test]
fn reader_handles_maximum_size_frames_back_to_back() {
    let bytes = [
        frame(0, 0xEE, 7, &[0; 249]),
        frame(0, 0xEE, 8, &[1; 249]),
        encode(&PING, 9),
    ]
    .concat();
    let results = read_all(&bytes);
    assert_eq!(results.len(), 3);
    assert_eq!(results[..2], [Err(FrameError::UnknownType(0xEE)); 2]);
    assert_eq!(results[2].map(|r| return r.sequence), Ok(9));
}
