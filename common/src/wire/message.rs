//! The messages of protocol.md §5, as owned Rust types with encode/decode.

use crate::domain::{Command, Flip, Sticks};
use crate::wire::codes::{EventCode, EventLevel, HelloResult, MessageType, ReasonCode, RequestType, Status};
use crate::wire::packet::{MAX_PACKET_LEN, MAX_PAYLOAD_LEN, Packet};

/// Why an intact packet is not a valid message. Unlike a `PacketError`, the sender can be told (protocol.md §5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Options {
    pub authentication: bool,
    pub unknown: UnknownOptions,
}

const KNOWN_OPTIONS: u32 = AUTHENTICATION;
const AUTHENTICATION: u32 = 1 << 0;

/// Option bits without a meaning yet, kept so the drone can answer "option not supported". Can't hold a known
/// bit, so every `Options` encodes and decodes back unchanged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct UnknownOptions(u32);

impl UnknownOptions {
    /// `None` if `bits` includes a known option.
    pub fn new(bits: u32) -> Option<Self> {
        return if bits & KNOWN_OPTIONS == 0 {
            Some(Self(bits))
        } else {
            None
        };
    }

    pub fn bits(self) -> u32 {
        return self.0;
    }
}

impl Options {
    fn from_bits(bits: u32) -> Self {
        return Self {
            authentication: bits & AUTHENTICATION != 0,
            unknown: UnknownOptions(bits & !KNOWN_OPTIONS),
        };
    }

    fn bits(&self) -> u32 {
        debug_assert_eq!(self.unknown.0 & KNOWN_OPTIONS, 0, "UnknownOptions holds a known bit");
        let authentication = if self.authentication { AUTHENTICATION } else { 0 };
        return authentication | self.unknown.0;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Hello {
    pub protocol_version: u8,
    pub options: Options,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HelloReply {
    pub protocol_version: u8,
    pub result: HelloResult,
    pub options: Options,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ping {
    pub timestamp: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Pong {
    pub ping_time: u32,
    pub receive_time: u32,
    pub send_time: u32,
}

/// Starts every request; matches answers to requests and detects resends (protocol.md §5.0). Every value is
/// valid: the client counts up with `wrapping_add`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RequestId(pub u16);

/// One of the commands of protocol.md §5.3; each has its own message type on the wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CommandRequest {
    pub request_id: RequestId,
    pub command: Command,
}

/// The drone's answer to a request it has no data message for, or that failed (protocol.md §5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Reply {
    pub request_id: RequestId,
    pub answers: RequestType,
    pub status: Status,
    pub reason: Option<ReasonCode>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Event {
    pub level: EventLevel,
    pub code: Option<EventCode>,
    pub text: EventText,
}

/// The rest of the payload after Level and Code.
pub const MAX_EVENT_TEXT_LEN: usize = MAX_PAYLOAD_LEN - 3;

/// UTF-8 text of at most `MAX_EVENT_TEXT_LEN` bytes, owned (protocol.md §7.1).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
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
        debug_assert!(self.len <= MAX_EVENT_TEXT_LEN, "length {}", self.len);
        debug_assert!(
            self.bytes[self.len..].iter().all(|&b| return b == 0),
            "PartialEq needs zero padding"
        );
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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
        let packet = self.write(sequence, out);
        debug_assert_eq!(Self::read(&packet), Ok(*self), "encoding doesn't decode back");
        return packet;
    }

    pub fn decode(packet: &Packet) -> Result<Self, MessageError> {
        let message = Self::read(packet);
        if let Ok(decoded) = message {
            debug_assert_eq!(
                decoded.write(packet.sequence(), &mut [0; MAX_PACKET_LEN]).as_bytes(),
                packet.as_bytes(),
                "{decoded:?} doesn't encode back to the bytes it came from"
            );
        }
        return message;
    }

    /// `encode` without its debug check, so the checks of `encode` and `decode` don't call each other.
    fn write<'a>(&self, sequence: u16, out: &'a mut [u8; MAX_PACKET_LEN]) -> Packet<'a> {
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
                w.u16(m.request_id.0);
                if let Command::Flip(flip) = m.command {
                    w.u16(flip.direction);
                    w.u16(flip.duration_ms);
                }
            }
            Message::Reply(m) => {
                w.u16(m.request_id.0);
                w.u8(u8::from(MessageType::from(m.answers)));
                w.u8(u8::from(m.status));
                w.u16(m.reason.map_or(0, u16::from));
            }
            Message::Event(m) => {
                w.u8(u8::from(m.level));
                w.u16(m.code.map_or(0, u16::from));
                w.put(m.text.as_str().as_bytes());
            }
        }
        let sequence = if is_handshake(self.message_type()) { 0 } else { sequence };
        return Packet::write(self.message_type(), sequence, w.written(), out);
    }

    fn read(packet: &Packet) -> Result<Self, MessageError> {
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
                answers: RequestType::try_from(MessageType::from(r.u8()?)).map_err(invalid)?,
                status: Status::try_from(r.u8()?).map_err(invalid)?,
                reason: optional_code(r.u16()?)?,
            }),
            MessageType::Event => Message::Event(Event {
                level: EventLevel::try_from(r.u8()?).map_err(invalid)?,
                code: optional_code(r.u16()?)?,
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

/// A two-byte code where 0 means "none".
fn optional_code<T: TryFrom<u16>>(value: u16) -> Result<Option<T>, MessageError> {
    return match value {
        0 => Ok(None),
        code => T::try_from(code).map(Some).map_err(invalid),
    };
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
        return Ok(RequestId(self.u16()?));
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
        debug_assert!(
            self.len + bytes.len() <= MAX_PAYLOAD_LEN,
            "{} + {} bytes",
            self.len,
            bytes.len()
        );
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
