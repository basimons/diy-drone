//! The messages of protocol.md §5, as owned Rust types with encode/decode.

use crate::domain::{Command, Flip, Sticks};
use crate::wire::codes::{HelloResult, MessageType, Unknown};
use crate::wire::event::Event;
use crate::wire::packet::{MAX_PACKET_LEN, Packet};
use crate::wire::payload::{Reader, Writer};
use crate::wire::reply::Reply;

/// Why an intact packet is not a valid message. Unlike a `PacketError`, the sender can be told (protocol.md §5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MessageError {
    UnknownType(Unknown<MessageType>),
    NotImplemented(MessageType), // a known type this build has no Rust message for yet
    WrongLength,                 // payload length doesn't match the message type
    InvalidValue,                // a field holds a value the protocol doesn't allow
}

impl core::fmt::Display for MessageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        return match self {
            MessageError::UnknownType(unknown) => write!(f, "unknown message type {:#04x}", unknown.value()),
            MessageError::NotImplemented(message_type) => {
                write!(f, "message type {message_type:?} is not implemented yet")
            }
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
            Message::Reply(m) => m.write(&mut w),
            Message::Event(m) => m.write(&mut w),
        }
        let sequence = if is_handshake(self.message_type()) { 0 } else { sequence };
        return Packet::write(self.message_type(), sequence, w.written(), out);
    }

    fn read(packet: &Packet) -> Result<Self, MessageError> {
        if is_handshake(packet.message_type()) && packet.sequence() != 0 {
            return Err(MessageError::InvalidValue);
        }
        let mut r = Reader::new(packet.payload());
        let message = match packet.message_type() {
            MessageType::Hello => Message::Hello(Hello {
                protocol_version: r.u8()?,
                options: Options::from_bits(r.u32()?),
            }),
            MessageType::HelloReply => Message::HelloReply(HelloReply {
                protocol_version: r.u8()?,
                result: HelloResult::from(r.u8()?),
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
            MessageType::Arm => command(&mut r, Command::Arm)?,
            MessageType::Disarm => command(&mut r, Command::Disarm)?,
            MessageType::TakeOff => command(&mut r, Command::TakeOff)?,
            MessageType::Land => command(&mut r, Command::Land)?,
            MessageType::GoHome => command(&mut r, Command::GoHome)?,
            MessageType::SteadyState => command(&mut r, Command::SteadyState)?,
            MessageType::ExitSteadyState => command(&mut r, Command::ExitSteadyState)?,
            MessageType::Flip => Message::Command(CommandRequest {
                request_id: RequestId(r.u16()?),
                command: Command::Flip(Flip {
                    direction: r.u16()?,
                    duration_ms: r.u16()?,
                }),
            }),
            MessageType::Reply => Message::Reply(Reply::read(&mut r)?),
            MessageType::Event => Message::Event(Event::read(&mut r)?),
            MessageType::Unknown(unknown) => return Err(MessageError::UnknownType(unknown)),
            other => return Err(MessageError::NotImplemented(other)),
        };
        if !r.is_empty() {
            return Err(MessageError::WrongLength);
        }
        return Ok(message);
    }
}

fn is_handshake(message_type: MessageType) -> bool {
    return matches!(message_type, MessageType::Hello | MessageType::HelloReply);
}

/// A command whose payload is only its Request ID.
fn command(r: &mut Reader, command: Command) -> Result<Message, MessageError> {
    return Ok(Message::Command(CommandRequest {
        request_id: RequestId(r.u16()?),
        command,
    }));
}
