//! Cutting a received byte stream into frames at the `0x00` delimiters, with `FrameSplitter`.
mod support;

use drone_common::wire::message::{DecodeError, MAX_ENCODED_LEN, Message, Ping, Received};
use drone_common::wire::splitter::FrameSplitter;
use support::*;

const PING: Message = Message::Ping(Ping { timestamp: 0x11223344 });

#[test]
fn returns_each_frame_including_its_delimiter() {
    let bytes = encode(&PING, 1);
    assert_eq!(split(&bytes, bytes.len()), vec![Ok(bytes)]);
}

#[test]
fn frames_split_across_chunks_come_out_whole() {
    let bytes = [encode(&PING, 1), encode(&PING, 2), encode(&PING, 3)].concat();
    let whole = split(&bytes, bytes.len());
    assert_eq!(whole.len(), 3);
    for chunk_size in 1..bytes.len() {
        assert_eq!(split(&bytes, chunk_size), whole, "chunks of {chunk_size}");
    }
}

#[test]
fn keeps_a_partial_frame_for_the_next_chunk() {
    let bytes = encode(&PING, 1);
    let (first, second) = bytes.split_at(4);
    let mut splitter = FrameSplitter::new();
    let mut chunk = first;
    assert!(splitter.next_frame(&mut chunk).is_none());
    assert!(chunk.is_empty());
    let mut chunk = second;
    assert_eq!(splitter.next_frame(&mut chunk), Some(Ok(&bytes[..])));
    assert!(splitter.next_frame(&mut chunk).is_none());
}

#[test]
fn returns_frames_sent_back_to_back() {
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
fn ignores_empty_frames() {
    let bytes = [vec![0, 0, 0], encode(&PING, 1), vec![0]].concat();
    assert_eq!(read_all(&bytes).len(), 1);
}

#[test]
fn recovers_after_garbage() {
    // Joining mid-frame: the partial frame fails, the next one is fine.
    let full = encode(&PING, 1);
    let results = read_all(&[full[3..].to_vec(), full.clone()].concat());
    assert_eq!(results.len(), 2);
    assert!(results[0].is_err());
    assert!(results[1].is_ok());
}

#[test]
fn reports_corruption_and_carries_on() {
    let mut corrupted = encode(&PING, 0x1234);
    corrupted[5] ^= 0x40; // a data byte, not a COBS code byte
    let results = read_all(&[corrupted, encode(&PING, 1)].concat());
    assert_eq!(results.len(), 2);
    assert_eq!(results[0], Err(DecodeError::BadCrc));
    assert!(results[1].is_ok());
}

#[test]
fn rejects_overlong_frame_and_carries_on() {
    let bytes = [vec![0x55; MAX_ENCODED_LEN + 10], vec![0], encode(&PING, 1)].concat();
    let results = read_all(&bytes);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0], Err(DecodeError::TooLong));
    assert!(results[1].is_ok());
}

#[test]
fn largest_frame_fits_in_any_chunk_size_and_one_more_byte_does_not() {
    let longest = [vec![0x55; MAX_ENCODED_LEN - 1], vec![0]].concat();
    let too_long = [vec![0x55; MAX_ENCODED_LEN], vec![0]].concat();
    let bytes = [longest.clone(), too_long, encode(&PING, 1)].concat();
    for chunk_size in 1..=bytes.len() {
        assert_eq!(
            split(&bytes, chunk_size),
            vec![Ok(longest.clone()), Err(DecodeError::TooLong), Ok(encode(&PING, 1))],
            "chunks of {chunk_size}"
        );
    }
}

#[test]
fn overflows_only_past_the_largest_possible_frame() {
    let longest = [vec![0x55; MAX_ENCODED_LEN - 1], vec![0]].concat();
    let too_long = [vec![0x55; MAX_ENCODED_LEN], vec![0]].concat();
    let results = read_all(&[longest, too_long, encode(&PING, 1)].concat());
    assert_eq!(results.len(), 3);
    assert!(
        matches!(results[0], Err(e) if e != DecodeError::TooLong),
        "{:?}",
        results[0]
    );
    assert_eq!(results[1], Err(DecodeError::TooLong));
    assert!(results[2].is_ok());
}

#[test]
fn rejects_frame_that_decodes_to_more_than_255_bytes() {
    // Each 0x01 block stands for one zero: 257 of them decode to 256 zeros.
    assert_eq!(
        read_all(&[vec![0x01; MAX_ENCODED_LEN - 1], vec![0]].concat()),
        vec![Err(DecodeError::TooLong)]
    );
}

#[test]
fn handles_maximum_size_frames_back_to_back() {
    let bytes = [
        frame(0, 0xEE, 7, &[0; 249]),
        frame(0, 0xEE, 8, &[1; 249]),
        encode(&PING, 9),
    ]
    .concat();
    let results = read_all(&bytes);
    assert_eq!(results.len(), 3);
    assert_eq!(results[..2], [Err(DecodeError::UnknownType(0xEE)); 2]);
    assert_eq!(results[2].map(|r| return r.sequence), Ok(9));
}

#[test]
fn overlong_frame_in_small_chunks_is_rejected_once() {
    let bytes = [vec![0x55; MAX_ENCODED_LEN + 10], vec![0], encode(&PING, 1)].concat();
    for chunk_size in [1, 7, 64, 300] {
        let frames = split(&bytes, chunk_size);
        assert_eq!(frames.len(), 2, "chunks of {chunk_size}");
        assert_eq!(frames[0], Err(DecodeError::TooLong), "chunks of {chunk_size}");
        assert_eq!(frames[1], Ok(encode(&PING, 1)), "chunks of {chunk_size}");
    }
}
