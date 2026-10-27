//! The messages of protocol.md §5, as owned Rust types with encode/decode.

use crate::wire::event::{EventId, EventKind};
use crate::wire::macros::{code, messages, wire_struct};
use crate::wire::packet::{MAX_PACKET_LEN, Packet};
use crate::wire::payload::{Code, Reader, Wire, Writer};
use crate::wire::reply::{Outcome, Reason, Status};

/// Why an intact packet is not a valid message. Unlike a `PacketError`, the sender can be told (protocol.md §5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MessageError {
    UnknownType(UnknownType),
    UnknownCode { kind: &'static str, value: u16 }, // e.g. kind "HelloResult": a value that code doesn't have
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
        return match TypeKind::of(value) {
            TypeKind::Unknown(unknown) => Some(unknown),
            TypeKind::Request(_) | TypeKind::Other => None,
        };
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

code!(
    /// Never changes between protocol versions, like the rest of Hello and HelloReply (protocol.md §5.1).
    pub HelloResult: u8 {
        Ok = 0,
        VersionMismatch = 1,
        OptionNotSupported = 2,
    }
);

wire_struct!(
    /// Starts every request; matches answers to requests and detects resends (protocol.md §5.0). Every value is
    /// valid: the client counts up with `wrapping_add`.
    RequestId(pub u16)
);

/// How old a TakeOff or Flip may be, by its Sent At, before the drone refuses it as `TooLate`; the client stops
/// resending it then too (protocol.md §5.3).
pub const COMMAND_EXPIRY_US: u32 = 300_000;

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
    fn answer(outcome: Outcome<Self::Refusal>) -> Answer;
}

messages! {
    others {
        Hello = 0x01 {
            protocol_version: u8,
            options: Options,
        },
        HelloReply = 0x02 {
            protocol_version: u8,
            result: HelloResult,
            options: Options,
        },
        Ping = 0x03 { timestamp: u32 },
        Pong = 0x04 {
            ping_time: u32,
            receive_time: u32,
            send_time: u32,
        },
        /// Which of the other side's messages arrived (protocol.md §5.0a).
        Ack = 0x08 {
            largest_received: u16, // the newest Sequence received
            ack_delay_us: u16,     // from receiving `largest_received` to sending this Ack
            received_before: u32,  // bit n set: Sequence `largest_received` − 1 − n arrived too
        },
        /// Stick positions, protocol-neutral (protocol.md §5.2): what they mean is up to the flight code. Also the
        /// internal state's type, as `domain::Sticks`.
        Sticks = 0x10 {
            throttle: u16, // 0 … 65535, no centre
            roll: i16,     // 0 = centred
            pitch: i16,
            yaw: i16,
        },
        /// The drone's answer to a request (protocol.md §5.4).
        Reply = 0x40 {
            request_id: RequestId,
            answer: Answer,
        },
        /// The drone tells the client something happened (protocol.md §5.4).
        Event = 0x41 {
            event_id: EventId,
            kind: EventKind,
        },
    }
    requests {
        /// Accepted only while the transmitter's arm switch is on (drone-design.md, "Failsafes").
        Arm = 0x20 {} refused ArmRefusal { BatteryLow, ThrottleNotLow, ImuNotReady, ArmSwitchOff },
        /// Never refused: disarming must always work.
        Disarm = 0x21 {},
        /// Expires: refused as `TooLate` when older than `COMMAND_EXPIRY_US`.
        TakeOff = 0x22 {
            sent_at: u32, // client's session clock in µs when the pilot gave the command; a resend keeps it
        } refused TakeOffRefusal { NotArmed, BatteryLow, TooLate },
        Land = 0x23 {} refused LandRefusal { NotFlying },
        /// "Not supported" until GPS is fitted.
        GoHome = 0x24 {},
        /// Expires like TakeOff.
        Flip = 0x25 {
            sent_at: u32, // like TakeOff
            direction: u16, // 0 = forward, clockwise seen from above, a full circle over 0 … 65535
            duration_ms: u16,
        } refused FlipRefusal { NotArmed, NotFlying, TooLow, TooLate },
        /// Hold still and accept only a few requests.
        SteadyState = 0x26 {} refused SteadyStateRefusal { NotFlying },
        ExitSteadyState = 0x27 {},
    }
}

impl Ack {
    /// Whether this Ack says that the message with this Sequence arrived: `largest_received` itself or one of the 32
    /// before it with its bit set.
    pub fn acknowledges(&self, sequence: u16) -> bool {
        return match self.largest_received.wrapping_sub(sequence) {
            0 => true,
            distance @ 1..=32 => self.received_before & (1 << (distance - 1)) != 0,
            _ => false,
        };
    }
}

impl Answer {
    fn from_wire(answers: u8, status: Status, reason: u16) -> Result<Self, MessageError> {
        return match TypeKind::of(answers) {
            TypeKind::Request(request) => match (status, reason) {
                (Status::Malformed, 0) => Ok(Answer::Malformed(request)),
                _ => request.outcome(status, reason),
            },
            TypeKind::Unknown(unknown) => match (status, reason) {
                (Status::UnknownType, 0) => Ok(Answer::UnknownType(unknown)),
                _ => Err(MessageError::InvalidValue),
            },
            TypeKind::Other => Err(MessageError::InvalidValue), // a known type that isn't a request
        };
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
