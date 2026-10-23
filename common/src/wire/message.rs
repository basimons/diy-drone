//! The messages of protocol.md §5, as owned Rust types with encode/decode.

use crate::wire::codes::{HelloResult, MessageType};
use crate::wire::envelope::{Envelope, MAX_PAYLOAD_LEN};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    UnknownType(u8), // answered with Reply "unknown type"
    WrongLength,
    InvalidValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hello {
    pub protocol_version: u16,
    pub options: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HelloReply {
    pub protocol_version: u16,
    pub result: HelloResult,
    pub options: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ping {
    pub timestamp: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pong {
    pub ping_time: u32,
    pub receive_time: u32,
    pub send_time: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message {
    Hello(Hello),
    HelloReply(HelloReply),
    Ping(Ping),
    Pong(Pong),
}

impl Message {
    pub fn message_type(&self) -> MessageType {
        return match self {
            Message::Hello(_) => MessageType::Hello,
            Message::HelloReply(_) => MessageType::HelloReply,
            Message::Ping(_) => MessageType::Ping,
            Message::Pong(_) => MessageType::Pong,
        };
    }

    /// Hello and HelloReply always get Sequence 0 (protocol.md §5.1), whatever `sequence` is.
    pub fn encode(&self, sequence: u16) -> Envelope {
        let mut w = Writer::new();
        match self {
            Message::Hello(m) => {
                w.u16(m.protocol_version);
                w.u32(m.options);
            }
            Message::HelloReply(m) => {
                w.u16(m.protocol_version);
                w.u8(u8::from(m.result));
                w.u32(m.options);
            }
            Message::Ping(m) => w.u32(m.timestamp),
            Message::Pong(m) => {
                w.u32(m.ping_time);
                w.u32(m.receive_time);
                w.u32(m.send_time);
            }
        }
        let sequence = if is_handshake(self.message_type()) { 0 } else { sequence };
        return Envelope::new(self.message_type(), sequence, w.written())
            .expect("every message fits in an envelope and has a known type");
    }

    pub fn decode(envelope: &Envelope) -> Result<Self, DecodeError> {
        if is_handshake(envelope.message_type()) && envelope.sequence() != 0 {
            return Err(DecodeError::InvalidValue);
        }
        let mut r = Reader {
            bytes: envelope.payload(),
        };
        let message = match envelope.message_type() {
            MessageType::Hello => Message::Hello(Hello {
                protocol_version: r.u16()?,
                options: r.u32()?,
            }),
            MessageType::HelloReply => Message::HelloReply(HelloReply {
                protocol_version: r.u16()?,
                result: HelloResult::try_from(r.u8()?).map_err(|_| return DecodeError::InvalidValue)?,
                options: r.u32()?,
            }),
            MessageType::Ping => Message::Ping(Ping { timestamp: r.u32()? }),
            MessageType::Pong => Message::Pong(Pong {
                ping_time: r.u32()?,
                receive_time: r.u32()?,
                send_time: r.u32()?,
            }),
            // Types without a Rust message yet are treated as unknown until they are added.
            other => return Err(DecodeError::UnknownType(u8::from(other))),
        };
        if !r.bytes.is_empty() {
            return Err(DecodeError::WrongLength);
        }
        return Ok(message);
    }
}

fn is_handshake(message_type: MessageType) -> bool {
    return matches!(message_type, MessageType::Hello | MessageType::HelloReply);
}

/// Reads little-endian fields from the front of a payload.
struct Reader<'a> {
    bytes: &'a [u8],
}

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let (head, rest) = self.bytes.split_first_chunk::<N>().ok_or(DecodeError::WrongLength)?;
        self.bytes = rest;
        return Ok(*head);
    }

    fn u8(&mut self) -> Result<u8, DecodeError> {
        return Ok(self.take::<1>()?[0]);
    }

    fn u16(&mut self) -> Result<u16, DecodeError> {
        return Ok(u16::from_le_bytes(self.take()?));
    }

    fn u32(&mut self) -> Result<u32, DecodeError> {
        return Ok(u32::from_le_bytes(self.take()?));
    }
}

/// Writes little-endian fields into a payload buffer.
struct Writer {
    bytes: [u8; MAX_PAYLOAD_LEN],
    len: usize,
}

impl Writer {
    fn new() -> Self {
        return Self {
            bytes: [0; MAX_PAYLOAD_LEN],
            len: 0,
        };
    }

    fn put(&mut self, bytes: &[u8]) {
        self.bytes[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
    }

    fn u8(&mut self, value: u8) {
        self.put(&[value]);
    }

    fn u16(&mut self, value: u16) {
        self.put(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.put(&value.to_le_bytes());
    }

    fn written(&self) -> &[u8] {
        return &self.bytes[..self.len];
    }
}
