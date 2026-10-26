//! Reply (protocol.md §5.4): the drone's answer to a request, typed per request.

use core::convert::Infallible;

use crate::wire::codes::{
    ArmRefusal, FlipRefusal, GetSettingsRefusal, LandRefusal, LinkTestRefusal, MessageType, ReadLogRefusal, Refusal,
    RequestOnceRefusal, SetSettingsRefusal, Status, SteadyStateRefusal, SubscribeRefusal, TakeOffRefusal, Unknown,
};
use crate::wire::message::{MessageError, RequestId};
use crate::wire::payload::{Reader, Writer};

/// The drone's answer to a request it has no data message for, or that failed (protocol.md §5.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Reply {
    pub request_id: RequestId,
    pub answer: Answer,
}

/// Why a request failed. On the wire: the Status, plus the Reason for `Refused`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Failure<R: Refusal> {
    Refused(R),
    Malformed,
    NotSupported,
    Busy,
    Unknown { status: Unknown<Status>, reason: u16 }, // a status this build doesn't know, kept for the caller
}

impl<R: Refusal> Failure<R> {
    fn to_wire(self) -> (Status, u16) {
        return match self {
            Failure::Refused(refusal) => (Status::Refused, refusal.reason()),
            Failure::Malformed => (Status::Malformed, 0),
            Failure::NotSupported => (Status::NotSupported, 0),
            Failure::Busy => (Status::Busy, 0),
            Failure::Unknown { status, reason } => (Status::Unknown(status), reason),
        };
    }

    fn from_wire(status: Status, reason: u16) -> Result<Self, MessageError> {
        return match (status, reason) {
            (Status::Refused, _) => R::from_reason(reason)
                .map(Failure::Refused)
                .ok_or(MessageError::InvalidValue),
            (Status::Malformed, 0) => Ok(Failure::Malformed),
            (Status::NotSupported, 0) => Ok(Failure::NotSupported),
            (Status::Busy, 0) => Ok(Failure::Busy),
            (Status::Unknown(status), _) => Ok(Failure::Unknown { status, reason }),
            _ => Err(MessageError::InvalidValue), // Ok or UnknownType, or a Reason where none belongs
        };
    }
}

fn outcome_to_wire<R: Refusal>(outcome: Result<(), Failure<R>>) -> (Status, u16) {
    return match outcome {
        Ok(()) => (Status::Ok, 0),
        Err(failure) => failure.to_wire(),
    };
}

fn outcome_from_wire<R: Refusal>(status: Status, reason: u16) -> Result<Result<(), Failure<R>>, MessageError> {
    return match (status, reason) {
        (Status::Ok, 0) => Ok(Ok(())),
        _ => Failure::from_wire(status, reason).map(Err),
    };
}

/// Defines `Answer`: per request, what its Reply can say.
/// `ok`: success is a Reply with Status OK. `failure_only`: success is a data message, so a Reply always failed.
macro_rules! answers {
    (ok { $($ok:ident($ok_refusal:ty),)* } failure_only { $($failed:ident($failed_refusal:ty),)* }) => {
        /// What a Reply says, typed per request (protocol.md §5.4), so a Reply can't combine Answers Type,
        /// Status and Reason in a way the protocol doesn't allow.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Answer {
            $($ok(Result<(), Failure<$ok_refusal>>),)*
            $($failed(Failure<$failed_refusal>),)*
            UnknownType(Unknown<MessageType>), // always Status "unknown type", never a Reason
        }

        impl Answer {
            /// Answers Type, Status and Reason.
            fn to_wire(self) -> (MessageType, (Status, u16)) {
                return match self {
                    $(Answer::$ok(outcome) => (MessageType::$ok, outcome_to_wire(outcome)),)*
                    $(Answer::$failed(failure) => (MessageType::$failed, failure.to_wire()),)*
                    Answer::UnknownType(unknown) => (MessageType::Unknown(unknown), (Status::UnknownType, 0)),
                };
            }

            fn from_wire(answers: MessageType, status: Status, reason: u16) -> Result<Self, MessageError> {
                return match answers {
                    $(MessageType::$ok => Ok(Answer::$ok(outcome_from_wire(status, reason)?)),)*
                    $(MessageType::$failed => Ok(Answer::$failed(Failure::from_wire(status, reason)?)),)*
                    MessageType::Unknown(unknown) => match (status, reason) {
                        (Status::UnknownType, 0) => Ok(Answer::UnknownType(unknown)),
                        _ => Err(MessageError::InvalidValue),
                    },
                    _ => Err(MessageError::InvalidValue), // not a request
                };
            }
        }
    };
}

answers! {
    ok {
        Arm(ArmRefusal),
        Disarm(Infallible), // never refused: disarming must always work
        TakeOff(TakeOffRefusal),
        Land(LandRefusal),
        GoHome(Infallible), // "not supported" until GPS is fitted
        Flip(FlipRefusal),
        SteadyState(SteadyStateRefusal),
        ExitSteadyState(Infallible),
        SetSettings(SetSettingsRefusal),
        Subscribe(SubscribeRefusal),
    }
    failure_only {
        LinkTest(LinkTestRefusal),       // success is LinkTestResult
        GetSettings(GetSettingsRefusal), // success is Settings
        RequestOnce(RequestOnceRefusal), // success is StreamData
        ListLogs(Infallible),            // success is LogList
        ReadLog(ReadLogRefusal),         // success is LogChunk
    }
}

impl Reply {
    pub(crate) fn write(&self, w: &mut Writer) {
        let (answers, (status, reason)) = self.answer.to_wire();
        w.u16(self.request_id.0);
        w.u8(u8::from(answers));
        w.u8(u8::from(status));
        w.u16(reason);
    }

    pub(crate) fn read(r: &mut Reader) -> Result<Self, MessageError> {
        return Ok(Self {
            request_id: RequestId(r.u16()?),
            answer: Answer::from_wire(MessageType::from(r.u8()?), Status::from(r.u8()?), r.u16()?)?,
        });
    }
}
