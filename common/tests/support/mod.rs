//! Test helpers that build frames by hand, independently of the library's own CRC and COBS code.
#![allow(dead_code)] // each test file uses a different subset

use drone_common::wire::message::{DecodeError, MAX_ENCODED_LEN, Message, Received};
use drone_common::wire::splitter::FrameSplitter;

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
    let mut out = [0u8; MAX_ENCODED_LEN];
    let len = message.encode(sequence, &mut out);
    return out[..len].to_vec();
}

/// Feeds `bytes` to one splitter in chunks of `chunk_size` and returns every frame it produced, in order.
pub fn split(bytes: &[u8], chunk_size: usize) -> Vec<Result<Vec<u8>, DecodeError>> {
    let mut splitter = FrameSplitter::new();
    let mut frames = Vec::new();
    for mut chunk in bytes.chunks(chunk_size) {
        while let Some(frame) = splitter.next_frame(&mut chunk) {
            frames.push(frame.map(|f| return f.to_vec()));
        }
        assert!(chunk.is_empty(), "next_frame returned None before using up the chunk");
    }
    return frames;
}

/// Splits `bytes` (as one chunk) and decodes every frame, the way a stream transport would.
pub fn read_all(bytes: &[u8]) -> Vec<Result<Received, DecodeError>> {
    let mut splitter = FrameSplitter::new();
    let mut chunk = bytes;
    let mut results = Vec::new();
    while let Some(frame) = splitter.next_frame(&mut chunk) {
        results.push(frame.and_then(Message::decode));
    }
    return results;
}
