//! The envelope around every message (protocol.md §4.1) and its COBS framing (§4.2).

use crate::wire::cobs;
use crate::wire::codes::MessageType;
use crate::wire::crc16::crc16;

const HEADER_LEN: usize = 4; // flags, type, sequence
const CRC_LEN: usize = 2;

pub const MAX_ENVELOPE_LEN: usize = 255;
pub const MAX_PAYLOAD_LEN: usize = MAX_ENVELOPE_LEN - HEADER_LEN - CRC_LEN;
/// COBS-encoded maximum envelope plus the `0x00` delimiter.
pub const MAX_ENCODED_LEN: usize = cobs::max_encoded_len(MAX_ENVELOPE_LEN) + 1;

/// Why received bytes were dropped (counted for link-quality stats).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    TooShort,
    TooLong,
    BadCobs, // not COBS data ending in a single 0x00, e.g. a code byte promises more bytes than there are
    BadCrc,
    UnsupportedFlags, // only allowed once authentication exists
    UnknownType(u8),  // kept so the receiver can still answer it (protocol.md §7.1)
    WrongLength,      // payload length doesn't match the message type
    InvalidValue,     // a field holds a value the protocol doesn't allow
}

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

    fn encode_raw(&self, out: &mut [u8; MAX_ENVELOPE_LEN]) -> usize {
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

    fn decode_raw(bytes: &[u8]) -> Result<Self, DecodeError> {
        if bytes.len() < HEADER_LEN + CRC_LEN {
            return Err(DecodeError::TooShort);
        }
        if bytes.len() > MAX_ENVELOPE_LEN {
            return Err(DecodeError::TooLong);
        }
        let (body, crc) = bytes.split_at(bytes.len() - CRC_LEN);
        if crc16(body) != u16::from_le_bytes([crc[0], crc[1]]) {
            return Err(DecodeError::BadCrc);
        }
        if body[0] != 0 {
            return Err(DecodeError::UnsupportedFlags);
        }
        return Ok(Self::from_parts(
            MessageType::from(body[1]),
            u16::from_le_bytes([body[2], body[3]]),
            &body[HEADER_LEN..],
        ));
    }

    /// COBS-encoded and with the `0x00` delimiter, ready to send. Returns the number of bytes written.
    pub fn encode(&self, out: &mut [u8; MAX_ENCODED_LEN]) -> usize {
        let mut raw = [0; MAX_ENVELOPE_LEN];
        let raw_len = self.encode_raw(&mut raw);
        let len =
            cobs::encode(&raw[..raw_len], &mut out[..MAX_ENCODED_LEN - 1]).expect("MAX_ENCODED_LEN fits any envelope");
        out[len] = 0;
        return len + 1;
    }

    /// `frame` is what `encode` writes: COBS data followed by one `0x00`.
    pub fn decode(frame: &[u8]) -> Result<Self, DecodeError> {
        let Some((&0, stuffed)) = frame.split_last() else {
            return Err(DecodeError::BadCobs);
        };
        let mut raw = [0; MAX_ENVELOPE_LEN];
        let len = cobs::decode(stuffed, &mut raw).map_err(|e| {
            return match e {
                cobs::CobsError::OutputTooSmall => DecodeError::TooLong,
                cobs::CobsError::ZeroByte | cobs::CobsError::Truncated => DecodeError::BadCobs,
            };
        })?;
        return Self::decode_raw(&raw[..len]);
    }
}
