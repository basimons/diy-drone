//! Checks the hand-written CRC-16 and COBS in `support/` against published values, so the frame tests can trust them.
mod support;

use support::{cobs_frame, crc16};

/// `count` consecutive byte values starting at `first`, e.g. run(0x01, 254) = 01 02 … FE.
fn run(first: u8, count: usize) -> Vec<u8> {
    return (0..count).map(|i| return first.wrapping_add(i as u8)).collect();
}

fn cat(parts: &[&[u8]]) -> Vec<u8> {
    return parts.concat();
}

/// The examples from the COBS article on Wikipedia, as (name, decoded, encoded).
fn examples() -> Vec<(&'static str, Vec<u8>, Vec<u8>)> {
    return vec![
        ("empty", vec![], vec![0x01]),
        ("single zero", vec![0x00], vec![0x01, 0x01]),
        ("two zeros", vec![0x00, 0x00], vec![0x01, 0x01, 0x01]),
        ("zero byte zero", vec![0x00, 0x11, 0x00], vec![0x01, 0x02, 0x11, 0x01]),
        (
            "zero in the middle",
            vec![0x11, 0x22, 0x00, 0x33],
            vec![0x03, 0x11, 0x22, 0x02, 0x33],
        ),
        (
            "no zeros",
            vec![0x11, 0x22, 0x33, 0x44],
            vec![0x05, 0x11, 0x22, 0x33, 0x44],
        ),
        (
            "trailing zeros",
            vec![0x11, 0x00, 0x00, 0x00],
            vec![0x02, 0x11, 0x01, 0x01, 0x01],
        ),
        ("254 non-zero bytes", run(0x01, 254), cat(&[&[0xFF], &run(0x01, 254)])),
        (
            "zero then 254 non-zero bytes",
            cat(&[&[0x00], &run(0x01, 254)]),
            cat(&[&[0x01, 0xFF], &run(0x01, 254)]),
        ),
        (
            "255 non-zero bytes",
            run(0x01, 255),
            cat(&[&[0xFF], &run(0x01, 254), &[0x02, 0xFF]]),
        ),
        (
            "254 non-zero bytes then zero",
            cat(&[&run(0x02, 254), &[0x00]]),
            cat(&[&[0xFF], &run(0x02, 254), &[0x01, 0x01]]),
        ),
        (
            "253 non-zero bytes, zero, one more",
            cat(&[&run(0x03, 253), &[0x00, 0x01]]),
            cat(&[&[0xFE], &run(0x03, 253), &[0x02, 0x01]]),
        ),
    ];
}

#[test]
fn crc16_matches_standard_check_value() {
    // The published check value for CRC-16/CCITT-FALSE over ASCII "123456789".
    assert_eq!(crc16(b"123456789"), 0x29B1);
}

#[test]
fn cobs_matches_wikipedia_examples() {
    for (name, decoded, encoded) in examples() {
        assert_eq!(cobs_frame(&decoded), [encoded, vec![0]].concat(), "{name}");
    }
}
