//! Reply (protocol.md §5.4): the drone's answer to a request, typed per request.

use crate::wire::codes::code;
use crate::wire::message::{Answer, MessageError, Request, RequestId};
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
}

/// Why a request failed. On the wire: the Status, plus the Reason for `Refused`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Failure<R> {
    Refused(R),
    Malformed,
    NotSupported,
    Busy,
    NotAllowedNow, // allowed in general, but not in the drone's current state (e.g. steady state)
}

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
        Err(Failure::Malformed) => (Status::Malformed, 0),
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
        (Status::Malformed, 0) => Ok(Err(Failure::Malformed)),
        (Status::NotSupported, 0) => Ok(Err(Failure::NotSupported)),
        (Status::Busy, 0) => Ok(Err(Failure::Busy)),
        (Status::NotAllowedNow, 0) => Ok(Err(Failure::NotAllowedNow)),
        (Status::UnknownType, 0) => Err(MessageError::InvalidValue), // only answers a type the drone doesn't know
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
