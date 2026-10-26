//! Test helpers that build frames by hand, independently of the library's own CRC and COBS code.
#![allow(dead_code)] // each test file uses a different subset

use drone_common::wire::frame::{FrameError, FrameReader, MAX_FRAME_LEN, Push, Received};
use drone_common::wire::message::Message;

/// CRC-16/CCITT-FALSE, written out bit by bit.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        for bit in (0..8).rev() {
            let feedback = ((crc >> 15) as u8 ^ (byte >> bit)) & 1 == 1;
            crc <<= 1;
            if feedback {
                crc ^= 0x1021;
            }
        }
    }
    return crc;
}

/// Plain COBS, followed by the `0x00` delimiter.
pub fn cobs_frame(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0];
    let mut code_at = 0;
    for (i, &byte) in data.iter().enumerate() {
        if byte != 0 {
            out.push(byte);
        }
        // A full block (254 data bytes) only opens a new one if more data follows.
        if byte == 0 || (out.len() - code_at == 255 && i + 1 < data.len()) {
            out[code_at] = (out.len() - code_at) as u8;
            code_at = out.len();
            out.push(0);
        }
    }
    out[code_at] = (out.len() - code_at) as u8;
    out.push(0);
    return out;
}

/// Raw envelope bytes (flags, type, sequence, payload) plus a correct CRC, COBS-framed.
pub fn frame(flags: u8, message_type: u8, sequence: u16, payload: &[u8]) -> Vec<u8> {
    let mut envelope = vec![flags, message_type];
    envelope.extend_from_slice(&sequence.to_le_bytes());
    envelope.extend_from_slice(payload);
    return with_crc_framed(envelope);
}

pub fn with_crc_framed(mut envelope: Vec<u8>) -> Vec<u8> {
    let crc = crc16(&envelope);
    envelope.extend_from_slice(&crc.to_le_bytes());
    return cobs_frame(&envelope);
}

pub fn encode(message: &Message, sequence: u16) -> Vec<u8> {
    let mut out = [0u8; MAX_FRAME_LEN];
    let len = message.encode_frame(sequence, &mut out);
    return out[..len].to_vec();
}

/// The result of every completed frame in `bytes`, in order.
pub fn read_all(bytes: &[u8]) -> Vec<Result<Received, FrameError>> {
    let mut reader = FrameReader::new();
    let mut results = Vec::new();
    for &byte in bytes {
        match reader.push(byte) {
            Push::NeedMore => {}
            Push::Frame(received) => results.push(Ok(received)),
            Push::Rejected(error) => results.push(Err(error)),
        }
    }
    return results;
}

pub fn read_one(bytes: &[u8]) -> Result<Received, FrameError> {
    let results = read_all(bytes);
    assert_eq!(results.len(), 1, "expected exactly one frame, got {results:?}");
    return results[0];
}
