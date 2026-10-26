//! The messages of protocol.md §5, as owned Rust types with encode/decode.

use crate::wire::codes::{HelloResult, MessageType};
use crate::wire::packet::{MAX_PACKET_LEN, MAX_PAYLOAD_LEN, Packet};

/// Why an intact packet is not a valid message. Unlike a `PacketError`, the sender can be told (protocol.md §5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageError {
    UnknownType(u8),
    WrongLength,  // payload length doesn't match the message type
    InvalidValue, // a field holds a value the protocol doesn't allow
}

impl core::fmt::Display for MessageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        return match self {
            MessageError::UnknownType(byte) => write!(f, "unknown message type {byte:#04x}"),
            MessageError::WrongLength => f.write_str("payload length doesn't match the message type"),
            MessageError::InvalidValue => f.write_str("a field holds a value the protocol doesn't allow"),
        };
    }
}

impl core::error::Error for MessageError {}

/// Hello/HelloReply options (protocol.md §5.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub authentication: bool,
    pub unknown: u32, // bits without a meaning yet, kept so the drone can answer "option not supported"
}

const AUTHENTICATION: u32 = 1 << 0;

impl Options {
    fn from_bits(bits: u32) -> Self {
        return Self {
            authentication: bits & AUTHENTICATION != 0,
            unknown: bits & !AUTHENTICATION,
        };
    }

    fn bits(&self) -> u32 {
        let authentication = if self.authentication { AUTHENTICATION } else { 0 };
        return authentication | (self.unknown & !AUTHENTICATION);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hello {
    pub protocol_version: u8,
    pub options: Options,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HelloReply {
    pub protocol_version: u8,
    pub result: HelloResult,
    pub options: Options,
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
    fn message_type(&self) -> MessageType {
        return match self {
            Message::Hello(_) => MessageType::Hello,
            Message::HelloReply(_) => MessageType::HelloReply,
            Message::Ping(_) => MessageType::Ping,
            Message::Pong(_) => MessageType::Pong,
        };
    }

    /// Writes the packet into `out`. Hello and HelloReply always get Sequence 0 (protocol.md §5.1), whatever
    /// `sequence` is.
    pub fn encode<'a>(&self, sequence: u16, out: &'a mut [u8; MAX_PACKET_LEN]) -> Packet<'a> {
        let mut w = Writer::new();
        match self {
            Message::Hello(m) => {
                w.u8(m.protocol_version);
                w.u32(m.options.bits());
            }
            Message::HelloReply(m) => {
                w.u8(m.protocol_version);
                w.u8(u8::from(m.result));
                w.u32(m.options.bits());
            }
            Message::Ping(m) => w.u32(m.timestamp),
            Message::Pong(m) => {
                w.u32(m.ping_time);
                w.u32(m.receive_time);
                w.u32(m.send_time);
            }
        }
        let sequence = if is_handshake(self.message_type()) { 0 } else { sequence };
        return Packet::write(self.message_type(), sequence, w.written(), out);
    }

    pub fn decode(packet: &Packet) -> Result<Self, MessageError> {
        if is_handshake(packet.message_type()) && packet.sequence() != 0 {
            return Err(MessageError::InvalidValue);
        }
        let mut r = Reader {
            bytes: packet.payload(),
        };
        let message = match packet.message_type() {
            MessageType::Hello => Message::Hello(Hello {
                protocol_version: r.u8()?,
                options: Options::from_bits(r.u32()?),
            }),
            MessageType::HelloReply => Message::HelloReply(HelloReply {
                protocol_version: r.u8()?,
                result: HelloResult::try_from(r.u8()?).map_err(|_| return MessageError::InvalidValue)?,
                options: Options::from_bits(r.u32()?),
            }),
            MessageType::Ping => Message::Ping(Ping { timestamp: r.u32()? }),
            MessageType::Pong => Message::Pong(Pong {
                ping_time: r.u32()?,
                receive_time: r.u32()?,
                send_time: r.u32()?,
            }),
            // Types without a Rust message yet are treated as unknown until they are added.
            other => return Err(MessageError::UnknownType(u8::from(other))),
        };
        if !r.bytes.is_empty() {
            return Err(MessageError::WrongLength);
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
    fn take<const N: usize>(&mut self) -> Result<[u8; N], MessageError> {
        let (head, rest) = self.bytes.split_first_chunk::<N>().ok_or(MessageError::WrongLength)?;
        self.bytes = rest;
        return Ok(*head);
    }

    fn u8(&mut self) -> Result<u8, MessageError> {
        return Ok(self.take::<1>()?[0]);
    }

    fn u32(&mut self) -> Result<u32, MessageError> {
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

    fn u32(&mut self, value: u32) {
        self.put(&value.to_le_bytes());
    }

    fn written(&self) -> &[u8] {
        return &self.bytes[..self.len];
    }
}
