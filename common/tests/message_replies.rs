mod support;

use core::convert::Infallible;
use core::num::NonZeroU16;
use drone_common::wire::codes::*;
use drone_common::wire::message::{Message, MessageError, RequestId};
use drone_common::wire::reply::{Answer, Failure, Reply};
use support::*;

const REPLY: u8 = 0x40;

fn reply(answer: Answer) -> Message {
    return Message::Reply(Reply {
        request_id: RequestId(0x0102),
        answer,
    });
}

/// Reply payload: Request ID 0x0102, then Answers Type, Status, Reason.
fn payload(answers: u8, status: u8, reason: u16) -> Vec<u8> {
    return [&[0x02, 0x01, answers, status][..], &reason.to_le_bytes()].concat();
}

#[test]
fn ok() {
    check_round_trip(reply(Answer::Arm(Ok(()))), REPLY, &payload(0x20, 0, 0));
    check_round_trip(reply(Answer::Subscribe(Ok(()))), REPLY, &payload(0x61, 0, 0));
}

#[test]
fn refused_with_its_reason() {
    let cases = [
        (Answer::Arm(Err(Failure::Refused(ArmRefusal::BatteryLow))), 0x20, 3),
        (
            Answer::TakeOff(Err(Failure::Refused(TakeOffRefusal::NotArmed))),
            0x22,
            1,
        ),
        (Answer::Land(Err(Failure::Refused(LandRefusal::NotFlying))), 0x23, 1),
        (Answer::Flip(Err(Failure::Refused(FlipRefusal::TooLow))), 0x25, 3),
        (
            Answer::SteadyState(Err(Failure::Refused(SteadyStateRefusal::NotFlying))),
            0x26,
            1,
        ),
        (
            Answer::LinkTest(Failure::Refused(LinkTestRefusal::NotAllowedNow)),
            0x05,
            1,
        ),
        (
            Answer::SetSettings(Err(Failure::Refused(SetSettingsRefusal::ValueOutOfRange))),
            0x50,
            2,
        ),
        (
            Answer::GetSettings(Failure::Refused(GetSettingsRefusal::UnknownGroup)),
            0x51,
            1,
        ),
        (
            Answer::RequestOnce(Failure::Refused(RequestOnceRefusal::UnknownStream)),
            0x60,
            1,
        ),
        (
            Answer::Subscribe(Err(Failure::Refused(SubscribeRefusal::RateNotPossible))),
            0x61,
            2,
        ),
        (
            Answer::ReadLog(Failure::Refused(ReadLogRefusal::OffsetPastEnd)),
            0x72,
            2,
        ),
    ];
    for (answer, answers, reason) in cases {
        check_round_trip(reply(answer), REPLY, &payload(answers, 1, reason));
    }
}

#[test]
fn failures_without_a_reason() {
    let cases: [(Failure<Infallible>, u8); 4] = [
        (Failure::Malformed, 3),
        (Failure::NotSupported, 4),
        (Failure::Busy, 5),
        (Failure::NotAllowedNow, 6), // e.g. in steady state
    ];
    for (failure, status) in cases {
        check_round_trip(reply(Answer::Disarm(Err(failure))), REPLY, &payload(0x21, status, 0));
        check_round_trip(reply(Answer::GoHome(Err(failure))), REPLY, &payload(0x24, status, 0));
        check_round_trip(
            reply(Answer::ExitSteadyState(Err(failure))),
            REPLY,
            &payload(0x27, status, 0),
        );
        check_round_trip(reply(Answer::ListLogs(failure)), REPLY, &payload(0x70, status, 0));
    }
}

#[test]
fn every_request_type_can_be_answered() {
    // Requests whose success is a data message can only fail in a Reply; Busy is used for those.
    let answers: [(Answer, u8, u8); 15] = [
        (Answer::LinkTest(Failure::Busy), 0x05, 5),
        (Answer::Arm(Ok(())), 0x20, 0),
        (Answer::Disarm(Ok(())), 0x21, 0),
        (Answer::TakeOff(Ok(())), 0x22, 0),
        (Answer::Land(Ok(())), 0x23, 0),
        (Answer::GoHome(Ok(())), 0x24, 0),
        (Answer::Flip(Ok(())), 0x25, 0),
        (Answer::SteadyState(Ok(())), 0x26, 0),
        (Answer::ExitSteadyState(Ok(())), 0x27, 0),
        (Answer::SetSettings(Ok(())), 0x50, 0),
        (Answer::GetSettings(Failure::Busy), 0x51, 5),
        (Answer::RequestOnce(Failure::Busy), 0x60, 5),
        (Answer::Subscribe(Ok(())), 0x61, 0),
        (Answer::ListLogs(Failure::Busy), 0x70, 5),
        (Answer::ReadLog(Failure::Busy), 0x72, 5),
    ];
    for (answer, answers, status) in answers {
        check_round_trip(reply(answer), REPLY, &payload(answers, status, 0));
    }
}

#[test]
fn unknown_type() {
    let answer = Answer::UnknownType(Unknown::new(0xEE).unwrap());
    check_round_trip(reply(answer), REPLY, &payload(0xEE, 2, 0));
}

#[test]
fn keeps_unknown_reason_and_status() {
    // A newer drone may send codes this client doesn't know; the caller decides what to do (e.g. log them).
    let reason = ArmRefusal::Unknown(Unknown::new(NonZeroU16::new(0x0999).unwrap()).unwrap());
    check_round_trip(
        reply(Answer::Arm(Err(Failure::Refused(reason)))),
        REPLY,
        &payload(0x20, 1, 0x0999),
    );
    let failure = Failure::Unknown {
        status: Unknown::new(9).unwrap(),
        reason: 7,
    };
    check_round_trip(reply(Answer::Flip(Err(failure))), REPLY, &payload(0x25, 9, 7));
    let failure = Failure::Unknown {
        status: Unknown::new(9).unwrap(),
        reason: 0,
    };
    check_round_trip(reply(Answer::ListLogs(failure)), REPLY, &payload(0x70, 9, 0));
}

#[test]
fn request_id_0_is_valid() {
    let message = Message::Reply(Reply {
        request_id: RequestId(0),
        answer: Answer::Land(Ok(())),
    });
    check_round_trip(message, REPLY, &[0, 0, 0x23, 0, 0, 0]);
}

#[test]
fn rejects_impossible_combinations() {
    let cases = [
        ("Ok with a reason", payload(0x20, 0, 1)),
        ("Refused without a reason", payload(0x20, 1, 0)),
        ("Busy with a reason", payload(0x20, 5, 1)),
        ("Malformed with a reason", payload(0x20, 3, 1)),
        ("Not supported with a reason", payload(0x20, 4, 1)),
        ("Not allowed now with a reason", payload(0x25, 6, 1)),
        ("Disarm refused", payload(0x21, 1, 1)),
        ("GoHome refused", payload(0x24, 1, 1)),
        ("ExitSteadyState refused", payload(0x27, 1, 1)),
        ("ListLogs refused", payload(0x70, 1, 1)),
        (
            "Ok to GetSettings, whose success is a Settings message",
            payload(0x51, 0, 0),
        ),
        ("Ok to ReadLog, whose success is a LogChunk", payload(0x72, 0, 0)),
        ("unknown type for a known request", payload(0x20, 2, 0)),
        ("unknown type answered Ok", payload(0xEE, 0, 0)),
        ("unknown type with a reason", payload(0xEE, 2, 1)),
        ("answers Sticks, not a request", payload(0x10, 0, 0)),
        ("answers Hello, not a request", payload(0x01, 0, 0)),
        ("answers a Reply, not a request", payload(0x40, 0, 0)),
    ];
    for (case, payload) in cases {
        assert_eq!(
            decode_payload(REPLY, &payload),
            Err(MessageError::InvalidValue),
            "{case}"
        );
    }
}

#[test]
fn rejects_wrong_length() {
    check_only_length(REPLY, &payload(0x20, 0, 0));
}

#[test]
fn every_reply_header_decodes_or_is_invalid() {
    // Every Answers Type × Status, with a few Reasons: decoding never panics (in debug builds this also runs the
    // re-encode check), and anything it accepts encodes back to the same bytes.
    for answers in 0..=255u8 {
        for status in 0..=255u8 {
            for reason in [0, 1, 2, 0x0999, u16::MAX] {
                let bytes = payload(answers, status, reason);
                match decode_payload(REPLY, &bytes) {
                    Ok(message) => assert_eq!(encode_packet(&message, 0), packet(0, REPLY, 0, &bytes)),
                    Err(error) => assert_eq!(error, MessageError::InvalidValue, "{bytes:?}"),
                }
            }
        }
    }
}
