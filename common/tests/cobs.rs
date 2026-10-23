use drone_common::wire::cobs::*;

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
fn encodes_wikipedia_examples() {
    for (name, decoded, encoded) in examples() {
        let mut out = [0u8; 300];
        let len = encode(&decoded, &mut out).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(&out[..len], &encoded[..], "{name}");
    }
}

#[test]
fn decodes_wikipedia_examples() {
    for (name, decoded, encoded) in examples() {
        let mut out = [0u8; 300];
        let len = decode(&encoded, &mut out).unwrap_or_else(|e| panic!("{name}: {e:?}"));
        assert_eq!(&out[..len], &decoded[..], "{name}");
    }
}

#[test]
fn round_trips_every_length_up_to_max_envelope() {
    // Pseudo-random bytes with plenty of zeros, so zeros land everywhere.
    let mut state: u32 = 1;
    for len in 0..=255 {
        let input: Vec<u8> = (0..len)
            .map(|_| {
                state = state.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let b = (state >> 16) as u8;
                if b < 40 { return 0 } else { return b }
            })
            .collect();
        let mut encoded = [0u8; 300];
        let enc_len = encode(&input, &mut encoded).unwrap();
        assert!(!encoded[..enc_len].contains(&0), "len {len}: encoded contains a zero");
        assert!(enc_len <= max_encoded_len(len), "len {len}: {enc_len} > max");
        let mut decoded = [0u8; 300];
        let dec_len = decode(&encoded[..enc_len], &mut decoded).unwrap();
        assert_eq!(&decoded[..dec_len], &input[..], "len {len}");
    }
}

#[test]
fn max_encoded_len_fits_a_full_envelope() {
    let mut out = [0u8; 300];
    let len = encode(&run(0x01, 255), &mut out).unwrap();
    assert_eq!(max_encoded_len(255), len);
}

#[test]
fn encode_rejects_too_small_output() {
    let mut out = [0u8; 4];
    assert_eq!(
        encode(&[0x11, 0x22, 0x33, 0x44], &mut out),
        Err(CobsError::OutputTooSmall)
    );
}

#[test]
fn decode_rejects_too_small_output() {
    let mut out = [0u8; 3];
    assert_eq!(
        decode(&[0x05, 0x11, 0x22, 0x33, 0x44], &mut out),
        Err(CobsError::OutputTooSmall)
    );
}

#[test]
fn decode_rejects_zero_byte() {
    let mut out = [0u8; 10];
    assert_eq!(decode(&[0x03, 0x11, 0x00], &mut out), Err(CobsError::ZeroByte));
    assert_eq!(decode(&[0x00], &mut out), Err(CobsError::ZeroByte));
}

#[test]
fn decode_rejects_truncated_input() {
    // Code byte 0x05 promises four data bytes, only two follow.
    let mut out = [0u8; 10];
    assert_eq!(decode(&[0x05, 0x11, 0x22], &mut out), Err(CobsError::Truncated));
}
