//! The messages of protocol.md §5, as owned Rust types with encode/decode.

use core::convert::Infallible;

use crate::domain::Sticks;
use crate::wire::codes::{ArmRefusal, FlipRefusal, HelloResult, LandRefusal, SteadyStateRefusal, TakeOffRefusal};
use crate::wire::event::Event;
use crate::wire::packet::{MAX_PACKET_LEN, Packet};
use crate::wire::payload::{Code, Reader, Wire, Writer, impl_wire};
use crate::wire::reply::{Failure, Reply, Status, outcome_from_wire, outcome_to_wire};

/// Why an intact packet is not a valid message. Unlike a `PacketError`, the sender can be told (protocol.md §5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MessageError {
    UnknownType(UnknownType),
    UnknownCode { kind: &'static str, value: u16 }, // e.g. kind "ArmRefusal": a value that code doesn't have
    WrongLength,                                    // payload length doesn't match the message type
    InvalidValue,                                   // a field holds a value the protocol doesn't allow
}

impl core::fmt::Display for MessageError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        return match self {
            MessageError::UnknownType(unknown) => write!(f, "unknown message type {:#04x}", unknown.value()),
            MessageError::UnknownCode { kind, value } => write!(f, "unknown {kind} {value}"),
            MessageError::WrongLength => f.write_str("payload length doesn't match the message type"),
            MessageError::InvalidValue => f.write_str("a field holds a value the protocol doesn't allow"),
        };
    }
}

impl core::error::Error for MessageError {}

/// A message type byte this build has no message for. Can't hold a known type, so the drone can only answer
/// "unknown type" to a type it really doesn't know (protocol.md §5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UnknownType(u8);

impl UnknownType {
    /// `None` if `value` is a message type this build knows.
    pub fn new(value: u8) -> Option<Self> {
        return if is_known_type(value) { None } else { Some(Self(value)) };
    }

    pub fn value(self) -> u8 {
        return self.0;
    }
}

/// Hello/HelloReply options (protocol.md §5.1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Options {
    pub authentication: bool,
    pub unknown: UnknownOptions,
}

const KNOWN_OPTIONS: u32 = AUTHENTICATION;
const AUTHENTICATION: u32 = 1 << 0;

/// Option bits without a meaning yet. Hello is read across protocol versions, and the drone answers these with
/// "option not supported", so they are kept. Can't hold a known bit, so every `Options` encodes and decodes back
/// unchanged.
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

impl Wire for Options {
    fn read(r: &mut Reader) -> Result<Self, MessageError> {
        let bits = u32::read(r)?;
        return Ok(Self {
            authentication: bits & AUTHENTICATION != 0,
            unknown: UnknownOptions(bits & !KNOWN_OPTIONS),
        });
    }

    fn write(&self, w: &mut Writer) {
        debug_assert_eq!(self.unknown.0 & KNOWN_OPTIONS, 0, "UnknownOptions holds a known bit");
        let authentication = if self.authentication { AUTHENTICATION } else { 0 };
        (authentication | self.unknown.0).write(w);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Hello {
    pub protocol_version: u8,
    pub options: Options,
}
impl_wire!(Hello {
    protocol_version,
    options
});

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct HelloReply {
    pub protocol_version: u8,
    pub result: HelloResult,
    pub options: Options,
}
impl_wire!(HelloReply {
    protocol_version,
    result,
    options
});

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Ping {
    pub timestamp: u32,
}
impl_wire!(Ping { timestamp });

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Pong {
    pub ping_time: u32,
    pub receive_time: u32,
    pub send_time: u32,
}
impl_wire!(Pong {
    ping_time,
    receive_time,
    send_time
});

impl_wire!(Sticks {
    throttle,
    roll,
    pitch,
    yaw
});

/// Starts every request; matches answers to requests and detects resends (protocol.md §5.0). Every value is
/// valid: the client counts up with `wrapping_add`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RequestId(pub u16);

impl Wire for RequestId {
    fn read(r: &mut Reader) -> Result<Self, MessageError> {
        return Ok(Self(u16::read(r)?));
    }

    fn write(&self, w: &mut Writer) {
        self.0.write(w);
    }
}

mod sealed {
    pub trait Sealed {}
}

/// A message the drone answers, with a Reply or a data message (protocol.md §5.0). Sealed: only the requests of
/// `messages!` implement it, so `answer` always gives the request's own `Answer` variant.
pub trait Request: sealed::Sealed {
    /// Why the drone may refuse it; `Infallible` if it never does.
    type Refusal: Code<Repr = u16>;

    fn request_id(&self) -> RequestId;

    /// The `Answer` variant for this request.
    fn answer(outcome: Result<(), Failure<Self::Refusal>>) -> Answer;
}

/// Defines a command whose payload is only its Request ID.
macro_rules! simple_commands {
    ($($(#[$meta:meta])* $name:ident,)*) => {$(
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $name {
            pub request_id: RequestId,
        }
        impl_wire!($name { request_id });
    )*};
}

simple_commands! {
    Arm,
    Disarm,
    TakeOff,
    Land,
    /// Not supported until GPS is fitted.
    GoHome,
    /// Hold still and accept only a few requests.
    SteadyState,
    ExitSteadyState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Flip {
    pub request_id: RequestId,
    pub direction: u16, // 0 = forward, clockwise seen from above, a full circle over 0 … 65535
    pub duration_ms: u16,
}
impl_wire!(Flip {
    request_id,
    direction,
    duration_ms
});

/// Defines `Message` and `Answer` from one list of message types, so a type byte, its message and (for a request)
/// its Answer can't get out of step.
macro_rules! messages {
    (
        others { $($name:ident = $type:literal,)* }
        requests { $($request:ident($refusal:ty) = $request_type:literal,)* }
    ) => {
        #[allow(clippy::large_enum_variant)] // owned messages are as big as the biggest one, on purpose (protocol.md §7.1)
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Message {
            $($name($name),)*
            $($request($request),)*
        }

        /// What a Reply says, typed per request (protocol.md §5.4), so a Reply can't combine Answers Type, Status
        /// and Reason in a way the protocol doesn't allow.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Answer {
            $($request(Result<(), Failure<$refusal>>),)*
            UnknownType(UnknownType), // always Status "unknown type", never a Reason
        }

        $(
            impl sealed::Sealed for $request {}

            impl Request for $request {
                type Refusal = $refusal;

                fn request_id(&self) -> RequestId {
                    return self.request_id;
                }

                fn answer(outcome: Result<(), Failure<$refusal>>) -> Answer {
                    return Answer::$request(outcome);
                }
            }
        )*

        fn is_known_type(value: u8) -> bool {
            return matches!(value, $($type)|* | $($request_type)|*);
        }

        impl Message {
            fn message_type(&self) -> u8 {
                return match self {
                    $(Message::$name(_) => $type,)*
                    $(Message::$request(_) => $request_type,)*
                };
            }

            fn read_payload(message_type: u8, r: &mut Reader) -> Result<Self, MessageError> {
                return match message_type {
                    $($type => Ok(Message::$name(Wire::read(r)?)),)*
                    $($request_type => Ok(Message::$request(Wire::read(r)?)),)*
                    _ => Err(MessageError::UnknownType(UnknownType(message_type))),
                };
            }

            fn write_payload(&self, w: &mut Writer) {
                match self {
                    $(Message::$name(m) => m.write(w),)*
                    $(Message::$request(m) => m.write(w),)*
                }
            }
        }

        impl Answer {
            /// Answers Type, Status and Reason.
            pub(crate) fn to_wire(self) -> (u8, (Status, u16)) {
                return match self {
                    $(Answer::$request(outcome) => ($request_type, outcome_to_wire(outcome)),)*
                    Answer::UnknownType(unknown) => (unknown.0, (Status::UnknownType, 0)),
                };
            }

            pub(crate) fn from_wire(answers: u8, status: Status, reason: u16) -> Result<Self, MessageError> {
                return match answers {
                    $($request_type => Ok(Answer::$request(outcome_from_wire(status, reason)?)),)*
                    _ => match (UnknownType::new(answers), status, reason) {
                        (Some(unknown), Status::UnknownType, 0) => Ok(Answer::UnknownType(unknown)),
                        _ => Err(MessageError::InvalidValue), // not a request, or not answered "unknown type"
                    },
                };
            }
        }
    };
}

messages! {
    others {
        Hello = 0x01,
        HelloReply = 0x02,
        Ping = 0x03,
        Pong = 0x04,
        Sticks = 0x10,
        Reply = 0x40,
        Event = 0x41,
    }
    requests {
        Arm(ArmRefusal) = 0x20,
        Disarm(Infallible) = 0x21, // never refused: disarming must always work
        TakeOff(TakeOffRefusal) = 0x22,
        Land(LandRefusal) = 0x23,
        GoHome(Infallible) = 0x24, // "not supported" until GPS is fitted
        Flip(FlipRefusal) = 0x25,
        SteadyState(SteadyStateRefusal) = 0x26,
        ExitSteadyState(Infallible) = 0x27,
    }
}

impl Message {
    /// Writes the packet into `out`. Hello and HelloReply are message 0 of a session (protocol.md §5.1), so for
    /// them `sequence` must be 0.
    pub fn encode<'a>(&self, sequence: u16, out: &'a mut [u8; MAX_PACKET_LEN]) -> Packet<'a> {
        debug_assert!(
            !self.is_handshake() || sequence == 0,
            "Hello and HelloReply are message 0 of a session, not message {sequence}"
        );
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

    fn is_handshake(&self) -> bool {
        return matches!(self, Message::Hello(_) | Message::HelloReply(_));
    }

    /// `encode` without its debug checks, so the checks of `encode` and `decode` don't call each other.
    fn write<'a>(&self, sequence: u16, out: &'a mut [u8; MAX_PACKET_LEN]) -> Packet<'a> {
        let mut w = Writer::new();
        self.write_payload(&mut w);
        return Packet::write(self.message_type(), sequence, w.written(), out);
    }

    fn read(packet: &Packet) -> Result<Self, MessageError> {
        let mut r = Reader::new(packet.payload());
        let message = Self::read_payload(packet.message_type(), &mut r)?;
        if !r.is_empty() {
            return Err(MessageError::WrongLength);
        }
        if message.is_handshake() && packet.sequence() != 0 {
            return Err(MessageError::InvalidValue);
        }
        return Ok(message);
    }
}
