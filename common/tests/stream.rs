//! Byte streams (protocol.md §4.2): COBS framing with `stream::encode` and `StreamDecoder`.
mod support;

use drone_common::wire::message::{Message, Ping, Pong};
use drone_common::wire::packet::{Packet, PacketError};
use drone_common::wire::stream::{self, MAX_FRAME_LEN, StreamDecoder};
use support::*;

const PING: Message = Message::Ping(Ping { timestamp: 0x11223344 });

#[test]
fn ping_frame_matches_bytes_computed_independently() {
    // Computed with a separate Python CRC-16/CCITT-FALSE and COBS.
    assert_eq!(
        encode_frame(&PING, 0x1234),
        [0x01, 0x0A, 0x03, 0x34, 0x12, 0x44, 0x33, 0x22, 0x11, 0x44, 0xDA, 0x00]
    );
    assert_eq!(
        encode_frame(&Message::Ping(Ping { timestamp: 0 }), 0),
        [0x01, 0x02, 0x03, 0x01, 0x01, 0x01, 0x01, 0x01, 0x03, 0xBC, 0xE9, 0x00]
    );
}

#[test]
fn library_agrees_with_the_test_helpers() {
    assert_eq!(
        encode_frame(&PING, 0x1234),
        frame(0, 0x03, 0x1234, &[0x44, 0x33, 0x22, 0x11])
    );
}

#[test]
fn pong_frame_is_20_bytes_with_only_a_trailing_zero() {
    // 18-byte packet + 2 for COBS overhead and delimiter (protocol.md §4.2).
    let pong = Message::Pong(Pong {
        ping_time: 0,
        receive_time: 0,
        send_time: 0,
    });
    let bytes = encode_frame(&pong, 0);
    assert_eq!(bytes.len(), 20);
    assert_eq!(bytes.last(), Some(&0));
    assert!(!bytes[..19].contains(&0));
}

#[test]
fn returns_the_packet_inside_a_frame() {
    let bytes = encode_frame(&PING, 1);
    assert_eq!(split(&bytes, bytes.len()), vec![Ok(encode_packet(&PING, 1))]);
}

#[test]
fn frames_split_across_chunks_come_out_whole() {
    let bytes = [encode_frame(&PING, 1), encode_frame(&PING, 2), encode_frame(&PING, 3)].concat();
    let whole = split(&bytes, bytes.len());
    assert_eq!(whole.len(), 3);
    for chunk_size in 1..bytes.len() {
        assert_eq!(split(&bytes, chunk_size), whole, "chunks of {chunk_size}");
    }
}

#[test]
fn keeps_a_partial_frame_for_the_next_chunk() {
    let bytes = encode_frame(&PING, 1);
    let (first, second) = bytes.split_at(4);
    let mut decoder = StreamDecoder::new();
    let mut chunk = first;
    assert!(decoder.next_packet(&mut chunk).is_none());
    assert!(chunk.is_empty());
    let mut chunk = second;
    let packet = decoder.next_packet(&mut chunk).unwrap().unwrap();
    assert_eq!(packet.as_bytes(), encode_packet(&PING, 1));
    assert!(decoder.next_packet(&mut chunk).is_none());
}

#[test]
fn returns_frames_sent_back_to_back() {
    let bytes = [encode_frame(&PING, 1), encode_frame(&PING, 2)].concat();
    assert_eq!(read_all(&bytes), vec![Ok((1, PING)), Ok((2, PING))]);
}

#[test]
fn ignores_empty_frames() {
    let bytes = [vec![0, 0, 0], encode_frame(&PING, 1), vec![0]].concat();
    assert_eq!(read_all(&bytes), vec![Ok((1, PING))]);
}

#[test]
fn recovers_after_garbage() {
    // Joining mid-frame: the partial frame fails, the next one is fine.
    let full = encode_frame(&PING, 1);
    let results = read_all(&[full[3..].to_vec(), full.clone()].concat());
    assert_eq!(results.len(), 2);
    assert!(results[0].is_err());
    assert_eq!(results[1], Ok((1, PING)));
}

#[test]
fn reports_corruption_and_carries_on() {
    let mut corrupted = encode_frame(&PING, 0x1234);
    corrupted[5] ^= 0x40; // a data byte, not a COBS code byte
    let results = read_all(&[corrupted, encode_frame(&PING, 1)].concat());
    assert_eq!(results, vec![Err(PacketError::BadCrc), Ok((1, PING))]);
}

#[test]
fn rejects_broken_cobs() {
    // Code byte 0x05 promises four data bytes, only two follow.
    assert_eq!(split(&[0x05, 0x11, 0x22, 0x00], 4), vec![Err(PacketError::BadCobs)]);
}

#[test]
fn rejects_frame_that_decodes_to_more_than_255_bytes() {
    // Each 0x01 block stands for one zero: 257 of them decode to 256 zeros.
    assert_eq!(
        split(&[vec![0x01; MAX_FRAME_LEN - 1], vec![0]].concat(), MAX_FRAME_LEN),
        vec![Err(PacketError::TooLong)]
    );
}

#[test]
fn largest_frame_fits_in_any_chunk_size_and_one_more_byte_does_not() {
    // The longest frame is still tried (and fails as COBS); one byte more is too long before decoding.
    let longest = [vec![0x55; MAX_FRAME_LEN - 1], vec![0]].concat();
    let too_long = [vec![0x55; MAX_FRAME_LEN], vec![0]].concat();
    let bytes = [longest, too_long, encode_frame(&PING, 1)].concat();
    for chunk_size in 1..=bytes.len() {
        assert_eq!(
            split(&bytes, chunk_size),
            vec![
                Err(PacketError::BadCobs),
                Err(PacketError::TooLong),
                Ok(encode_packet(&PING, 1))
            ],
            "chunks of {chunk_size}"
        );
    }
}

#[test]
fn overlong_frame_in_small_chunks_is_rejected_once() {
    let bytes = [vec![0x55; MAX_FRAME_LEN + 10], vec![0], encode_frame(&PING, 1)].concat();
    for chunk_size in [1, 7, 64, 300] {
        assert_eq!(
            split(&bytes, chunk_size),
            vec![Err(PacketError::TooLong), Ok(encode_packet(&PING, 1))],
            "chunks of {chunk_size}"
        );
    }
}

/// A sequence number that leaves no zero after the flags byte of the packet `flags, 0xEE, sequence, 249 × 0x01`.
fn sequence_without_zeros(flags: u8) -> u16 {
    return (0x0101..=u16::MAX)
        .find(|&sequence| {
            let bytes = packet(flags, 0xEE, sequence, &[1; 249]);
            return !bytes[1..].contains(&0);
        })
        .unwrap();
}

#[test]
fn decodes_a_254_byte_run_that_ends_the_frame() {
    // After the flags byte, all 254 bytes are non-zero: one full COBS block (code 0xFF) and nothing after it.
    let sequence = sequence_without_zeros(0);
    let bytes = frame(0, 0xEE, sequence, &[1; 249]);
    assert_eq!(bytes[1], 0xFF);
    assert_eq!(
        split(&bytes, bytes.len()),
        vec![Ok(packet(0, 0xEE, sequence, &[1; 249]))]
    );
}

#[test]
fn decodes_a_254_byte_run_followed_by_more_data() {
    // Flags 0x01 makes all 255 bytes non-zero: a full block, then a second block with the last byte.
    // Flags are checked after COBS and CRC, so UnsupportedFlags shows the frame got that far.
    let bytes = frame(0x01, 0xEE, sequence_without_zeros(0x01), &[1; 249]);
    assert_eq!(bytes.len(), MAX_FRAME_LEN);
    assert_eq!(bytes[0], 0xFF);
    assert_eq!(split(&bytes, bytes.len()), vec![Err(PacketError::UnsupportedFlags)]);
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
        let bytes = frame(0, 0xEE, 0, &payload);
        assert_eq!(
            split(&bytes, bytes.len()),
            vec![Ok(packet(0, 0xEE, 0, &payload))],
            "payload of {len} bytes"
        );
    }
}

#[test]
fn maximum_size_frames_back_to_back() {
    let bytes = [
        frame(0, 0xEE, 7, &[0; 249]),
        frame(0, 0xEE, 8, &[1; 249]),
        encode_frame(&PING, 9),
    ]
    .concat();
    assert_eq!(
        split(&bytes, bytes.len()),
        vec![
            Ok(packet(0, 0xEE, 7, &[0; 249])),
            Ok(packet(0, 0xEE, 8, &[1; 249])),
            Ok(encode_packet(&PING, 9))
        ]
    );
}

#[test]
fn library_encoder_matches_the_helper_on_maximum_size_packets() {
    // Packets this big don't exist as messages yet, so build them by hand and only use the stream layer.
    let mut out = [0u8; MAX_FRAME_LEN];
    for payload in [[0u8; 249], [1u8; 249]] {
        let bytes = packet(0, 0xEE, 0x0101, &payload);
        let encoded = stream::encode(&Packet::parse(&bytes).unwrap(), &mut out).to_vec();
        assert_eq!(encoded, cobs_frame(&bytes), "payload {}", payload[0]);
        assert_eq!(split(&encoded, 7), vec![Ok(bytes)], "payload {}", payload[0]);
    }
}

#[test]
fn never_panics_on_garbage() {
    let mut state: u32 = 3;
    let bytes: Vec<u8> = (0..20_000)
        .map(|_| {
            state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
            let b = (state >> 16) as u8;
            if b < 8 { return 0 } else { return b }
        })
        .collect();
    for chunk_size in [1, 13, 300] {
        let _ = split(&bytes, chunk_size);
    }
}
