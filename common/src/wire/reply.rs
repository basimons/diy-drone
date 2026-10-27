//! Reply (protocol.md §5.4): the drone's answer to a request, typed per request.

use crate::wire::macros::code;
use crate::wire::message::{Answer, Message, MessageError, Request, RequestId, TypeKind};
use crate::wire::packet::Packet;
use crate::wire::payload::{Code, Reader, Wire, Writer};
use core::convert::Infallible;

/// The drone's answer to a request (protocol.md §5.4). Only built as the answer to a request (`Reply::to`,
/// `CommandMessage::reply`) or to a packet that didn't decode (`Reply::to_invalid`), or decoded, so its Answer is
/// always for the request with its Request ID.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Reply {
    request_id: RequestId,
    answer: Answer,
}

impl Reply {
    pub fn request_id(&self) -> RequestId {
        return self.request_id;
    }

    pub fn answer(&self) -> Answer {
        return self.answer;
    }

    /// The Reply to `request`: its Request ID and type come from the request, so they can't be mixed up.
    pub fn to<R: Request>(request: &R, outcome: Outcome<R::Refusal>) -> Self {
        let answer = R::answer(outcome);
        debug_assert!(
            !matches!(answer, Answer::Malformed(_) | Answer::UnknownType(_)),
            "a request that decoded got {answer:?}"
        );
        return Self {
            request_id: request.request_id(),
            answer,
        };
    }

    /// The Reply to a packet `Message::decode` rejected: "malformed" for a request, "unknown type" for a type this
    /// build doesn't know. `None` if the packet decodes, isn't a request, or is too short to hold a Request ID.
    /// Drone side only: the client never sends Replies.
    pub fn to_invalid(packet: &Packet) -> Option<Self> {
        if Message::decode(packet).is_ok() {
            return None; // a valid request never gets "malformed"
        }
        let answer = match TypeKind::of(packet.message_type()) {
            TypeKind::Request(request) => Answer::Malformed(request),
            TypeKind::Unknown(unknown) => Answer::UnknownType(unknown),
            TypeKind::Other => return None, // not a request: nothing to answer
        };
        return Some(Self {
            request_id: RequestId::read(&mut Reader::new(packet.payload())).ok()?,
            answer,
        });
    }
}

impl Wire for Reply {
    fn read(r: &mut Reader) -> Result<Self, MessageError> {
        return Ok(Self {
            request_id: RequestId::read(r)?,
            answer: Answer::read(r)?,
        });
    }

    fn write(&self, w: &mut Writer) {
        self.request_id.write(w);
        self.answer.write(w);
    }
}

/// What came of a request that decoded (protocol.md §5.4). On the wire: the Status, plus the Reason for
/// `Refused`. A request that didn't decode is `Answer::Malformed` instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Outcome<R> {
    Ok,
    Refused(R),
    NotSupported,
    Busy,
    NotAllowedNow, // allowed in general, but not in the drone's current state (e.g. steady state)
}

impl Outcome<Infallible> {
    /// The same outcome for a request that may be refused: without a refusal, it fits every request.
    pub(crate) fn widen<R>(self) -> Outcome<R> {
        return match self {
            Outcome::Ok => Outcome::Ok,
            Outcome::Refused(never) => match never {},
            Outcome::NotSupported => Outcome::NotSupported,
            Outcome::Busy => Outcome::Busy,
            Outcome::NotAllowedNow => Outcome::NotAllowedNow,
        };
    }
}

impl<R: Code<Repr = u16>> Outcome<R> {
    /// Status and Reason.
    pub(crate) fn write(self, w: &mut Writer) {
        let (status, reason) = match self {
            Outcome::Ok => (Status::Ok, 0),
            Outcome::Refused(refusal) => (Status::Refused, refusal.encode()),
            Outcome::NotSupported => (Status::NotSupported, 0),
            Outcome::Busy => (Status::Busy, 0),
            Outcome::NotAllowedNow => (Status::NotAllowedNow, 0),
        };
        status.write(w);
        reason.write(w);
    }

    /// Only a refusal has a Reason, and then never 0.
    pub(crate) fn from_wire(status: Status, reason: u16) -> Result<Self, MessageError> {
        return match (status, reason) {
            (Status::Refused, 1..) => Ok(Outcome::Refused(R::decode(reason)?)),
            (Status::Ok, 0) => Ok(Outcome::Ok),
            (Status::NotSupported, 0) => Ok(Outcome::NotSupported),
            (Status::Busy, 0) => Ok(Outcome::Busy),
            (Status::NotAllowedNow, 0) => Ok(Outcome::NotAllowedNow),
            _ => Err(MessageError::InvalidValue), // also Malformed and UnknownType, which aren't per request
        };
    }
}

code!(
    /// Why a request was refused: one table for all requests (protocol.md §5.4). Each request has its own enum
    /// of the reasons it may use (e.g. `FlipRefusal`), which converts into this one. Never 0: Reason 0 means
    /// "no reason".
    pub Reason: u16 {
        NotArmed = 1,
        NotFlying = 2,
        TooLow = 3,
        BatteryLow = 4,
        ThrottleNotLow = 5,
        ImuNotReady = 6,       // IMU: the motion sensor
        ArmSwitchOff = 7,      // the transmitter's arm switch is not in the armed position
        TooLate = 14,          // the command is older than its expiry (protocol.md §5.3)
        AttitudeNotReady = 15, // no attitude estimate yet, which angle mode needs
        AlreadyFlying = 16,
    }
);

code!(
    /// Status byte of a Reply. In Rust a Reply holds an `Answer` instead, which can't combine them wrongly.
    pub(crate) Status: u8 {
        Ok = 0,
        Refused = 1,
        UnknownType = 2,
        Malformed = 3,
        NotSupported = 4,
        Busy = 5,
        NotAllowedNow = 6,
    }
);
