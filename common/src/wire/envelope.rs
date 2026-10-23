//! The envelope around every message (protocol.md §4.1) and its COBS framing (§4.2).

use crate::wire::cobs::{self, CobsError};
use crate::wire::crc16::crc16;

const HEADER_LEN: usize = 4; // flags, type, sequence
const CRC_LEN: usize = 2;

pub const MAX_ENVELOPE_LEN: usize = 255;
pub const MAX_PAYLOAD_LEN: usize = MAX_ENVELOPE_LEN - HEADER_LEN - CRC_LEN;
/// COBS-encoded maximum envelope plus the `0x00` delimiter.
pub const MAX_FRAME_LEN: usize = cobs::max_encoded_len(MAX_ENVELOPE_LEN) + 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvelopeError {
    PayloadTooLong,
    TooShort,
    TooLong,
    BadCrc,
    UnsupportedFlags, // only allowed once authentication exists
    Cobs(CobsError),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Envelope {
    /// Raw, not `MessageType`: an unknown type must still reach the drone so it can answer "unknown type".
    pub message_type: u8,
    pub sequence: u16,
    payload: [u8; MAX_PAYLOAD_LEN],
    payload_len: u8,
}

impl Envelope {
    pub fn new(message_type: u8, sequence: u16, payload: &[u8]) -> Result<Self, EnvelopeError> {
        if payload.len() > MAX_PAYLOAD_LEN {
            return Err(EnvelopeError::PayloadTooLong);
        }
        let mut envelope = Self {
            message_type,
            sequence,
            payload: [0; MAX_PAYLOAD_LEN],
            payload_len: payload.len() as u8,
        };
        envelope.payload[..payload.len()].copy_from_slice(payload);
        return Ok(envelope);
    }

    pub fn payload(&self) -> &[u8] {
        return &self.payload[..usize::from(self.payload_len)];
    }

    /// Returns the number of bytes written.
    pub fn encode(&self, out: &mut [u8; MAX_ENVELOPE_LEN]) -> usize {
        let payload = self.payload();
        let crc_at = HEADER_LEN + payload.len();
        out[0] = 0; // flags
        out[1] = self.message_type;
        out[2..4].copy_from_slice(&self.sequence.to_le_bytes());
        out[HEADER_LEN..crc_at].copy_from_slice(payload);
        let crc = crc16(&out[..crc_at]);
        out[crc_at..crc_at + CRC_LEN].copy_from_slice(&crc.to_le_bytes());
        return crc_at + CRC_LEN;
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, EnvelopeError> {
        if bytes.len() < HEADER_LEN + CRC_LEN {
            return Err(EnvelopeError::TooShort);
        }
        if bytes.len() > MAX_ENVELOPE_LEN {
            return Err(EnvelopeError::TooLong);
        }
        let (body, crc) = bytes.split_at(bytes.len() - CRC_LEN);
        if crc16(body) != u16::from_le_bytes([crc[0], crc[1]]) {
            return Err(EnvelopeError::BadCrc);
        }
        if body[0] != 0 {
            return Err(EnvelopeError::UnsupportedFlags);
        }
        return Self::new(body[1], u16::from_le_bytes([body[2], body[3]]), &body[HEADER_LEN..]);
    }

    /// COBS-encoded and with the `0x00` delimiter, ready to send. Returns the number of bytes written.
    pub fn encode_frame(&self, out: &mut [u8; MAX_FRAME_LEN]) -> usize {
        let mut raw = [0; MAX_ENVELOPE_LEN];
        let raw_len = self.encode(&mut raw);
        let len =
            cobs::encode(&raw[..raw_len], &mut out[..MAX_FRAME_LEN - 1]).expect("MAX_FRAME_LEN fits any envelope");
        out[len] = 0;
        return len + 1;
    }

    /// `frame` is COBS-encoded, without its `0x00` delimiter.
    fn decode_frame(frame: &[u8]) -> Result<Self, EnvelopeError> {
        let mut raw = [0; MAX_ENVELOPE_LEN];
        let len = cobs::decode(frame, &mut raw).map_err(|e| {
            return match e {
                CobsError::OutputTooSmall => EnvelopeError::TooLong,
                other => EnvelopeError::Cobs(other),
            };
        })?;
        return Self::decode(&raw[..len]);
    }
}

/// Splits a received byte stream into envelopes at the `0x00` delimiters.
pub struct FrameReader {
    buffer: [u8; MAX_FRAME_LEN - 1], // the delimiter is not stored
    len: usize,
    overflowed: bool,
}

impl FrameReader {
    pub fn new() -> Self {
        return Self {
            buffer: [0; MAX_FRAME_LEN - 1],
            len: 0,
            overflowed: false,
        };
    }

    /// Returns a result when `byte` completes a frame. Empty frames (repeated delimiters) are skipped.
    pub fn push(&mut self, byte: u8) -> Option<Result<Envelope, EnvelopeError>> {
        if byte == 0 {
            let result = if self.overflowed {
                Some(Err(EnvelopeError::TooLong))
            } else if self.len == 0 {
                None
            } else {
                Some(Envelope::decode_frame(&self.buffer[..self.len]))
            };
            self.len = 0;
            self.overflowed = false;
            return result;
        }
        if self.len == self.buffer.len() {
            self.overflowed = true;
        } else {
            self.buffer[self.len] = byte;
            self.len += 1;
        }
        return None;
    }
}

impl Default for FrameReader {
    fn default() -> Self {
        return Self::new();
    }
}
