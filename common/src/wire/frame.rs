//! Turning a received byte stream into messages (protocol.md §4). Sending is `Message::encode_frame`.

use crate::wire::cobs;
use crate::wire::envelope::{Envelope, MAX_ENVELOPE_LEN};
use crate::wire::message::Message;

/// COBS-encoded maximum envelope plus the `0x00` delimiter: the buffer size for `Message::encode_frame`.
pub const MAX_FRAME_LEN: usize = cobs::max_encoded_len(MAX_ENVELOPE_LEN) + 1;

/// Why a received frame was dropped (counted for link-quality stats).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameError {
    TooShort,
    TooLong,
    BadCobs, // the stuffing is broken, e.g. a code byte promises more bytes than the frame has
    BadCrc,
    UnsupportedFlags, // only allowed once authentication exists
    UnknownType(u8),  // kept so the receiver can still answer it (protocol.md §7.1)
    WrongLength,      // payload length doesn't match the message type
    InvalidValue,     // a field holds a value the protocol doesn't allow
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Received {
    pub sequence: u16,
    pub message: Message,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Push {
    NeedMore, // the frame isn't complete yet
    Frame(Received),
    Rejected(FrameError),
}

/// Splits a received byte stream into messages at the `0x00` delimiters.
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

    /// Feed one received byte. Empty frames (repeated delimiters) are skipped.
    pub fn push(&mut self, byte: u8) -> Push {
        if byte == 0 {
            let result = if self.overflowed {
                Push::Rejected(FrameError::TooLong)
            } else if self.len == 0 {
                Push::NeedMore
            } else {
                match decode(&self.buffer[..self.len]) {
                    Ok(received) => Push::Frame(received),
                    Err(error) => Push::Rejected(error),
                }
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
        return Push::NeedMore;
    }
}

impl Default for FrameReader {
    fn default() -> Self {
        return Self::new();
    }
}

/// `frame` is COBS-encoded, without its `0x00` delimiter.
fn decode(frame: &[u8]) -> Result<Received, FrameError> {
    let mut raw = [0; MAX_ENVELOPE_LEN];
    let len = cobs::decode(frame, &mut raw).map_err(|e| {
        return match e {
            cobs::CobsError::OutputTooSmall => FrameError::TooLong,
            cobs::CobsError::ZeroByte | cobs::CobsError::Truncated => FrameError::BadCobs,
        };
    })?;
    let envelope = Envelope::decode(&raw[..len])?;
    return Ok(Received {
        sequence: envelope.sequence(),
        message: Message::decode(&envelope)?,
    });
}
