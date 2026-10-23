//! COBS (Consistent Overhead Byte Stuffing, protocol.md §4.2): rewrites a frame so it
//! contains no `0x00` bytes, so `0x00` can mark where one frame ends and the next begins.
//!
//! These functions only do the byte stuffing; adding and finding the `0x00` delimiter
//! is up to the framing code.

/// Why encoding or decoding failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CobsError {
    /// The output buffer is too small for the result.
    OutputTooSmall,
    /// Encoded data contained a `0x00` byte, which COBS never produces.
    ZeroByte,
    /// A code byte promised more bytes than the input has.
    Truncated,
}

/// The largest encoded size for `len` input bytes: one extra byte per started run of 254
/// (and one for empty input).
pub const fn max_encoded_len(len: usize) -> usize {
    let runs = len.div_ceil(254);
    return len + if runs == 0 { 1 } else { runs };
}

/// Encodes `input` into `output`, returning the number of bytes written.
pub fn encode(input: &[u8], output: &mut [u8]) -> Result<usize, CobsError> {
    // Each block is a code byte followed by up to 254 non-zero bytes. The code byte says
    // how far it is to the next zero (or block end), so it is filled in when the block ends.
    if output.is_empty() {
        return Err(CobsError::OutputTooSmall);
    }
    let mut code_at = 0;
    let mut written = 1;
    let mut code: u8 = 1;
    for &byte in input {
        if code == 0xFF {
            // A full block of 254 non-zero bytes: close it, with no implied zero.
            output[code_at] = code;
            code_at = written;
            put(output, &mut written, 0)?;
            code = 1;
        }
        if byte == 0 {
            output[code_at] = code;
            code_at = written;
            put(output, &mut written, 0)?;
            code = 1;
        } else {
            put(output, &mut written, byte)?;
            code += 1;
        }
    }
    output[code_at] = code;
    return Ok(written);
}

/// Decodes one frame (without its `0x00` delimiter) into `output`, returning the
/// number of bytes written.
pub fn decode(input: &[u8], output: &mut [u8]) -> Result<usize, CobsError> {
    let mut read = 0;
    let mut written = 0;
    while read < input.len() {
        let code = input[read];
        if code == 0 {
            return Err(CobsError::ZeroByte);
        }
        read += 1;
        let block_end = read + usize::from(code) - 1;
        let block = input.get(read..block_end).ok_or(CobsError::Truncated)?;
        for &byte in block {
            if byte == 0 {
                return Err(CobsError::ZeroByte);
            }
            put(output, &mut written, byte)?;
        }
        read = block_end;
        // Every block except a full one (0xFF) and the last one stood for a zero.
        if code != 0xFF && read < input.len() {
            put(output, &mut written, 0)?;
        }
    }
    return Ok(written);
}

/// Writes `byte` at `output[*at]` and moves `at` on, or fails if `output` is full.
fn put(output: &mut [u8], at: &mut usize, byte: u8) -> Result<(), CobsError> {
    *output.get_mut(*at).ok_or(CobsError::OutputTooSmall)? = byte;
    *at += 1;
    return Ok(());
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{vec, vec::Vec};

    /// `count` consecutive byte values starting at `first`, e.g. run(0x01, 254) = 01 02 … FE.
    fn run(first: u8, count: usize) -> Vec<u8> {
        return (0..count)
            .map(|i| return first.wrapping_add(i as u8))
            .collect();
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
            (
                "zero byte zero",
                vec![0x00, 0x11, 0x00],
                vec![0x01, 0x02, 0x11, 0x01],
            ),
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
            (
                "254 non-zero bytes",
                run(0x01, 254),
                cat(&[&[0xFF], &run(0x01, 254)]),
            ),
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
            assert!(
                !encoded[..enc_len].contains(&0),
                "len {len}: encoded contains a zero"
            );
            assert!(
                enc_len <= max_encoded_len(len),
                "len {len}: {enc_len} > max"
            );
            let mut decoded = [0u8; 300];
            let dec_len = decode(&encoded[..enc_len], &mut decoded).unwrap();
            assert_eq!(&decoded[..dec_len], &input[..], "len {len}");
        }
    }

    #[test]
    fn max_encoded_len_fits_a_full_envelope() {
        // 255 non-zero bytes is the worst case for a maximum-size envelope.
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
        assert_eq!(
            decode(&[0x03, 0x11, 0x00], &mut out),
            Err(CobsError::ZeroByte)
        );
        assert_eq!(decode(&[0x00], &mut out), Err(CobsError::ZeroByte));
    }

    #[test]
    fn decode_rejects_truncated_input() {
        // Code byte 0x05 promises four data bytes, only two follow.
        let mut out = [0u8; 10];
        assert_eq!(
            decode(&[0x05, 0x11, 0x22], &mut out),
            Err(CobsError::Truncated)
        );
    }
}
