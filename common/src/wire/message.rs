//! The messages of protocol.md §5, as owned Rust types with encode/decode.

use core::num::NonZeroU16;

use crate::domain::{Command, Flip, Sticks};
use crate::wire::codes::{EventCode, EventLevel, HelloResult, MessageType, ReasonCode, Status};
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

/// Starts every request; matches answers to requests and detects resends (protocol.md §5.0). Never 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RequestId(NonZeroU16);

impl RequestId {
    pub fn new(id: u16) -> Option<Self> {
        return NonZeroU16::new(id).map(Self);
    }

    pub fn get(self) -> u16 {
        return self.0.get();
    }
}

/// One of the commands of protocol.md §5.3; each has its own message type on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandRequest {
    pub request_id: RequestId,
    pub command: Command,
}

/// The drone's answer to a request it has no data message for, or that failed (protocol.md §5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reply {
    pub request_id: RequestId,
    pub answers: MessageType, // the type of the request; `Unknown` when answering a type we don't know
    pub status: Status,
    pub reason: ReasonCode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub level: EventLevel,
    pub code: EventCode,
    pub text: EventText,
}

/// The rest of the payload after Level and Code.
pub const MAX_EVENT_TEXT_LEN: usize = MAX_PAYLOAD_LEN - 3;

/// UTF-8 text of at most `MAX_EVENT_TEXT_LEN` bytes, owned (protocol.md §7.1).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EventText {
    bytes: [u8; MAX_EVENT_TEXT_LEN], // zero after `len`, so the derived `PartialEq` compares the text only
    len: usize,
}

impl EventText {
    /// `None` if `text` is longer than `MAX_EVENT_TEXT_LEN` bytes.
    pub fn new(text: &str) -> Option<Self> {
        if text.len() > MAX_EVENT_TEXT_LEN {
            return None;
        }
        return Some(Self {
            bytes: core::array::from_fn(|i| return text.as_bytes().get(i).copied().unwrap_or(0)),
            len: text.len(),
        });
    }

    pub fn as_str(&self) -> &str {
        return core::str::from_utf8(&self.bytes[..self.len]).expect("only built from a str");
    }
}

impl Default for EventText {
    fn default() -> Self {
        return Self {
            bytes: [0; MAX_EVENT_TEXT_LEN],
            len: 0,
        };
    }
}

impl core::fmt::Debug for EventText {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        return core::fmt::Debug::fmt(self.as_str(), f);
    }
}

#[allow(clippy::large_enum_variant)] // owned messages are as big as the biggest one, on purpose (protocol.md §7.1)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Message {
    Hello(Hello),
    HelloReply(HelloReply),
    Ping(Ping),
    Pong(Pong),
    Sticks(Sticks),
    Command(CommandRequest),
    Reply(Reply),
    Event(Event),
}

impl Message {
    fn message_type(&self) -> MessageType {
        return match self {
            Message::Hello(_) => MessageType::Hello,
            Message::HelloReply(_) => MessageType::HelloReply,
            Message::Ping(_) => MessageType::Ping,
            Message::Pong(_) => MessageType::Pong,
            Message::Sticks(_) => MessageType::Sticks,
            Message::Command(m) => match m.command {
                Command::Arm => MessageType::Arm,
                Command::Disarm => MessageType::Disarm,
                Command::TakeOff => MessageType::TakeOff,
                Command::Land => MessageType::Land,
                Command::GoHome => MessageType::GoHome,
                Command::Flip(_) => MessageType::Flip,
                Command::SteadyState => MessageType::SteadyState,
                Command::ExitSteadyState => MessageType::ExitSteadyState,
            },
            Message::Reply(_) => MessageType::Reply,
            Message::Event(_) => MessageType::Event,
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
            Message::Sticks(m) => {
                w.u16(m.throttle);
                w.i16(m.roll);
                w.i16(m.pitch);
                w.i16(m.yaw);
            }
            Message::Command(m) => {
                w.u16(m.request_id.get());
                if let Command::Flip(flip) = m.command {
                    w.u16(flip.direction);
                    w.u16(flip.duration_ms);
                }
            }
            Message::Reply(m) => {
                w.u16(m.request_id.get());
                w.u8(u8::from(m.answers));
                w.u8(u8::from(m.status));
                w.u16(m.reason.0);
            }
            Message::Event(m) => {
                w.u8(u8::from(m.level));
                w.u16(m.code.0);
                w.put(m.text.as_str().as_bytes());
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
                result: HelloResult::try_from(r.u8()?).map_err(invalid)?,
                options: Options::from_bits(r.u32()?),
            }),
            MessageType::Ping => Message::Ping(Ping { timestamp: r.u32()? }),
            MessageType::Pong => Message::Pong(Pong {
                ping_time: r.u32()?,
                receive_time: r.u32()?,
                send_time: r.u32()?,
            }),
            MessageType::Sticks => Message::Sticks(Sticks {
                throttle: r.u16()?,
                roll: r.i16()?,
                pitch: r.i16()?,
                yaw: r.i16()?,
            }),
            MessageType::Arm => r.command(Command::Arm)?,
            MessageType::Disarm => r.command(Command::Disarm)?,
            MessageType::TakeOff => r.command(Command::TakeOff)?,
            MessageType::Land => r.command(Command::Land)?,
            MessageType::GoHome => r.command(Command::GoHome)?,
            MessageType::SteadyState => r.command(Command::SteadyState)?,
            MessageType::ExitSteadyState => r.command(Command::ExitSteadyState)?,
            MessageType::Flip => Message::Command(CommandRequest {
                request_id: r.request_id()?,
                command: Command::Flip(Flip {
                    direction: r.u16()?,
                    duration_ms: r.u16()?,
                }),
            }),
            MessageType::Reply => Message::Reply(Reply {
                request_id: r.request_id()?,
                answers: MessageType::from(r.u8()?),
                status: Status::try_from(r.u8()?).map_err(invalid)?,
                reason: ReasonCode(r.u16()?),
            }),
            MessageType::Event => Message::Event(Event {
                level: EventLevel::try_from(r.u8()?).map_err(invalid)?,
                code: EventCode(r.u16()?),
                text: EventText::new(core::str::from_utf8(r.rest()).map_err(invalid)?)
                    .ok_or(MessageError::WrongLength)?,
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

fn invalid<E>(_: E) -> MessageError {
    return MessageError::InvalidValue;
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

    fn u16(&mut self) -> Result<u16, MessageError> {
        return Ok(u16::from_le_bytes(self.take()?));
    }

    fn i16(&mut self) -> Result<i16, MessageError> {
        return Ok(i16::from_le_bytes(self.take()?));
    }

    fn u32(&mut self) -> Result<u32, MessageError> {
        return Ok(u32::from_le_bytes(self.take()?));
    }

    fn rest(&mut self) -> &[u8] {
        return core::mem::take(&mut self.bytes);
    }

    fn request_id(&mut self) -> Result<RequestId, MessageError> {
        return RequestId::new(self.u16()?).ok_or(MessageError::InvalidValue);
    }

    /// A command whose payload is only its Request ID.
    fn command(&mut self, command: Command) -> Result<Message, MessageError> {
        return Ok(Message::Command(CommandRequest {
            request_id: self.request_id()?,
            command,
        }));
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

    fn i16(&mut self, value: i16) {
        self.put(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.put(&value.to_le_bytes());
    }

    fn written(&self) -> &[u8] {
        return &self.bytes[..self.len];
    }
}
