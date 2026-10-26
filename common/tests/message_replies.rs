mod support;

use core::convert::Infallible;
use drone_common::wire::codes::*;
use drone_common::wire::message::*;
use drone_common::wire::reply::{Failure, Reply};
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
fn every_request_type_can_be_answered_ok() {
    let answers: [(Answer, u8); 8] = [
        (Answer::Arm(Ok(())), 0x20),
        (Answer::Disarm(Ok(())), 0x21),
        (Answer::TakeOff(Ok(())), 0x22),
        (Answer::Land(Ok(())), 0x23),
        (Answer::GoHome(Ok(())), 0x24),
        (Answer::Flip(Ok(())), 0x25),
        (Answer::SteadyState(Ok(())), 0x26),
        (Answer::ExitSteadyState(Ok(())), 0x27),
    ];
    for (answer, answers) in answers {
        check_round_trip(reply(answer), REPLY, &payload(answers, 0, 0));
    }
}

#[test]
fn every_refusal_with_its_reason() {
    fn arm(r: ArmRefusal) -> Answer {
        return Answer::Arm(Err(Failure::Refused(r)));
    }
    fn take_off(r: TakeOffRefusal) -> Answer {
        return Answer::TakeOff(Err(Failure::Refused(r)));
    }
    fn flip(r: FlipRefusal) -> Answer {
        return Answer::Flip(Err(Failure::Refused(r)));
    }
    let cases = [
        (arm(ArmRefusal::ThrottleNotLow), 0x20, 1),
        (arm(ArmRefusal::ImuNotReady), 0x20, 2),
        (arm(ArmRefusal::BatteryLow), 0x20, 3),
        (take_off(TakeOffRefusal::NotArmed), 0x22, 1),
        (take_off(TakeOffRefusal::BatteryLow), 0x22, 2),
        (Answer::Land(Err(Failure::Refused(LandRefusal::NotFlying))), 0x23, 1),
        (flip(FlipRefusal::NotArmed), 0x25, 1),
        (flip(FlipRefusal::NotFlying), 0x25, 2),
        (flip(FlipRefusal::TooLow), 0x25, 3),
        (
            Answer::SteadyState(Err(Failure::Refused(SteadyStateRefusal::NotFlying))),
            0x26,
            1,
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
    }
}

#[test]
fn reply_to_takes_request_id_and_type_from_the_request() {
    let flip = Flip {
        request_id: RequestId(9),
        direction: 0,
        duration_ms: 300,
    };
    let refused = Err(Failure::Refused(FlipRefusal::TooLow));
    assert_eq!(
        Reply::to(&flip, refused),
        Reply {
            request_id: RequestId(9),
            answer: Answer::Flip(refused),
        }
    );
    let arm = Arm {
        request_id: RequestId(10),
    };
    assert_eq!(Reply::to(&arm, Ok(())).answer, Answer::Arm(Ok(())));
}

#[test]
fn unknown_type() {
    let answer = Answer::UnknownType(UnknownType::new(0xEE).unwrap());
    check_round_trip(reply(answer), REPLY, &payload(0xEE, 2, 0));
}

#[test]
fn unknown_type_never_holds_a_known_type() {
    assert_eq!(UnknownType::new(0x20), None); // Arm
    assert_eq!(UnknownType::new(0x40), None); // Reply
    assert_eq!(UnknownType::new(0xEE).map(|t| return t.value()), Some(0xEE));
    // In the protocol, but without a Rust message yet: this build answers it with "unknown type".
    assert_eq!(UnknownType::new(0x05).map(|t| return t.value()), Some(0x05)); // LinkTest
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
fn rejects_unknown_codes() {
    // The protocol version is bumped when codes are added, so after Hello an unknown code is an error.
    let unknown_reason = MessageError::UnknownCode {
        kind: "ArmRefusal",
        value: 9,
    };
    assert_eq!(decode_payload(REPLY, &payload(0x20, 1, 9)), Err(unknown_reason));
    let unknown_status = MessageError::UnknownCode {
        kind: "Status",
        value: 9,
    };
    assert_eq!(decode_payload(REPLY, &payload(0x25, 9, 0)), Err(unknown_status));
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
        ("unknown type for a known request", payload(0x20, 2, 0)),
        ("unknown type answered Ok", payload(0xEE, 0, 0)),
        ("unknown type refused", payload(0xEE, 1, 1)),
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
fn every_reply_header_decodes_or_is_rejected() {
    // Every Answers Type × Status, with a few Reasons: decoding never panics (in debug builds this also runs the
    // re-encode check), and anything it accepts encodes back to the same bytes.
    for answers in 0..=255u8 {
        for status in 0..=255u8 {
            for reason in [0, 1, 2, 0x0999, u16::MAX] {
                let bytes = payload(answers, status, reason);
                match decode_payload(REPLY, &bytes) {
                    Ok(message) => assert_eq!(encode_packet(&message, 0), packet(0, REPLY, 0, &bytes)),
                    Err(error) => assert!(
                        matches!(error, MessageError::InvalidValue | MessageError::UnknownCode { .. }),
                        "{bytes:?}: {error:?}"
                    ),
                }
            }
        }
    }
}
