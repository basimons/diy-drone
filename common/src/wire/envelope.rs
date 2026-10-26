//! The envelope around every message (protocol.md §4.1) and its COBS framing (§4.2).

use crate::wire::cobs;
use crate::wire::codes::MessageType;
use crate::wire::crc16::crc16;
use crate::wire::frame::{FrameError, MAX_FRAME_LEN};

const HEADER_LEN: usize = 4; // flags, type, sequence
const CRC_LEN: usize = 2;

pub const MAX_ENVELOPE_LEN: usize = 255;
pub const MAX_PAYLOAD_LEN: usize = MAX_ENVELOPE_LEN - HEADER_LEN - CRC_LEN;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Envelope {
    message_type: MessageType,
    sequence: u16,
    payload: [u8; MAX_PAYLOAD_LEN],
    payload_len: u8,
}

impl Envelope {
    pub fn new(message_type: MessageType, sequence: u16, payload: &[u8]) -> Self {
        // `Unknown` only comes out of decoding; sending it would decode as a different type.
        debug_assert!(!matches!(message_type, MessageType::Unknown(_)), "{message_type:?}");
        debug_assert!(payload.len() <= MAX_PAYLOAD_LEN, "payload of {} bytes", payload.len());
        return Self::from_parts(message_type, sequence, payload);
    }

    /// `payload` must be at most `MAX_PAYLOAD_LEN` bytes.
    fn from_parts(message_type: MessageType, sequence: u16, payload: &[u8]) -> Self {
        let mut envelope = Self {
            message_type,
            sequence,
            payload: [0; MAX_PAYLOAD_LEN],
            payload_len: payload.len() as u8,
        };
        envelope.payload[..payload.len()].copy_from_slice(payload);
        return envelope;
    }

    pub fn message_type(&self) -> MessageType {
        return self.message_type;
    }

    pub fn sequence(&self) -> u16 {
        return self.sequence;
    }

    pub fn payload(&self) -> &[u8] {
        return &self.payload[..usize::from(self.payload_len)];
    }

    /// Returns the number of bytes written.
    pub fn encode(&self, out: &mut [u8; MAX_ENVELOPE_LEN]) -> usize {
        let payload = self.payload();
        let crc_at = HEADER_LEN + payload.len();
        out[0] = 0; // flags
        out[1] = u8::from(self.message_type);
        out[2..4].copy_from_slice(&self.sequence.to_le_bytes());
        out[HEADER_LEN..crc_at].copy_from_slice(payload);
        let crc = crc16(&out[..crc_at]);
        out[crc_at..crc_at + CRC_LEN].copy_from_slice(&crc.to_le_bytes());
        return crc_at + CRC_LEN;
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, FrameError> {
        if bytes.len() < HEADER_LEN + CRC_LEN {
            return Err(FrameError::TooShort);
        }
        if bytes.len() > MAX_ENVELOPE_LEN {
            return Err(FrameError::TooLong);
        }
        let (body, crc) = bytes.split_at(bytes.len() - CRC_LEN);
        if crc16(body) != u16::from_le_bytes([crc[0], crc[1]]) {
            return Err(FrameError::BadCrc);
        }
        if body[0] != 0 {
            return Err(FrameError::UnsupportedFlags);
        }
        return Ok(Self::from_parts(
            MessageType::from(body[1]),
            u16::from_le_bytes([body[2], body[3]]),
            &body[HEADER_LEN..],
        ));
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
}
