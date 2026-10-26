//! Reply (protocol.md §5.4): the drone's answer to a request, typed per request.

use crate::wire::macros::code;
use crate::wire::message::{Answer, Message, MessageError, Request, RequestId};
use crate::wire::packet::Packet;
use crate::wire::payload::{Code, Reader, Wire, Writer};

/// The drone's answer to a request (protocol.md §5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Reply {
    pub request_id: RequestId,
    pub answer: Answer,
}

impl Reply {
    /// The Reply to `request`: its Request ID and type come from the request, so they can't be mixed up.
    pub fn to<R: Request>(request: &R, outcome: Result<(), Failure<R::Refusal>>) -> Self {
        return Self {
            request_id: request.request_id(),
            answer: R::answer(outcome),
        };
    }

    /// The Reply to a packet `Message::decode` rejected: "malformed" for a request, "unknown type" for a type this
    /// build doesn't know. `None` if the packet decodes, isn't a request, or is too short to hold a Request ID.
    /// Drone side only: the client never sends Replies.
    pub fn to_invalid(packet: &Packet) -> Option<Self> {
        if Message::decode(packet).is_ok() {
            return None;
        }
        return Some(Self {
            request_id: RequestId::read(&mut Reader::new(packet.payload())).ok()?,
            answer: Answer::to_invalid(packet.message_type())?,
        });
    }
}

/// Why a decoded request failed. On the wire: the Status, plus the Reason for `Refused`. A request that didn't
/// decode is `Answer::Malformed`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Failure<R> {
    Refused(R),
    NotSupported,
    Busy,
    NotAllowedNow, // allowed in general, but not in the drone's current state (e.g. steady state)
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
        ImuNotReady = 6, // IMU: the motion sensor
        ArmSwitchOff = 7, // the transmitter's arm switch is not in the armed position
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

/// Status and Reason of a request's outcome.
pub(crate) fn outcome_to_wire<R: Code<Repr = u16>>(outcome: Result<(), Failure<R>>) -> (Status, u16) {
    let (status, reason) = match outcome {
        Ok(()) => (Status::Ok, 0),
        Err(Failure::Refused(refusal)) => (Status::Refused, refusal.repr()),
        Err(Failure::NotSupported) => (Status::NotSupported, 0),
        Err(Failure::Busy) => (Status::Busy, 0),
        Err(Failure::NotAllowedNow) => (Status::NotAllowedNow, 0),
    };
    debug_assert!(
        (status == Status::Refused) == (reason != 0),
        "{status:?} with Reason {reason}"
    );
    return (status, reason);
}

pub(crate) fn outcome_from_wire<R: Code<Repr = u16>>(
    status: Status,
    reason: u16,
) -> Result<Result<(), Failure<R>>, MessageError> {
    return match (status, reason) {
        (Status::Refused, 0) => Err(MessageError::InvalidValue),
        (Status::Refused, _) => Ok(Err(Failure::Refused(R::from_repr(reason)?))),
        (_, 1..) => Err(MessageError::InvalidValue), // only a refusal has a Reason
        (Status::Ok, 0) => Ok(Ok(())),
        (Status::NotSupported, 0) => Ok(Err(Failure::NotSupported)),
        (Status::Busy, 0) => Ok(Err(Failure::Busy)),
        (Status::NotAllowedNow, 0) => Ok(Err(Failure::NotAllowedNow)),
        (Status::Malformed | Status::UnknownType, 0) => Err(MessageError::InvalidValue), // not per request
    };
}

impl Wire for Reply {
    fn read(r: &mut Reader) -> Result<Self, MessageError> {
        return Ok(Self {
            request_id: RequestId::read(r)?,
            answer: Answer::from_wire(u8::read(r)?, Status::read(r)?, u16::read(r)?)?,
        });
    }

    fn write(&self, w: &mut Writer) {
        let (answers, (status, reason)) = self.answer.to_wire();
        self.request_id.write(w);
        answers.write(w);
        status.write(w);
        reason.write(w);
    }
}
