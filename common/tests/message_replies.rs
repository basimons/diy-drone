mod support;

use core::convert::Infallible;
use drone_common::wire::message::*;
use drone_common::wire::packet::Packet;
use drone_common::wire::reply::{Outcome, Reason, Reply};
use support::*;

const REPLY: u8 = 0x40;

/// Replies can't be built by hand: decodes `payload`, checks it says `answer` to Request ID 0x0102, and checks it
/// encodes back to the same bytes.
fn check_reply(answer: Answer, payload: &[u8]) {
    let Ok(Message::Reply(reply)) = decode_payload(REPLY, payload) else {
        panic!("{payload:?} is not a Reply");
    };
    assert_eq!((reply.request_id(), reply.answer()), (RequestId(0x0102), answer));
    check_round_trip(Message::Reply(reply), REPLY, payload);
}

/// (Request ID, Answer) of a reply, for comparing.
fn parts(reply: Option<Reply>) -> Option<(RequestId, Answer)> {
    return reply.map(|reply| return (reply.request_id(), reply.answer()));
}

/// Reply payload: Request ID 0x0102, then Answers Type, Status, Reason.
fn payload(answers: u8, status: u8, reason: u16) -> Vec<u8> {
    return [&[0x02, 0x01, answers, status][..], &reason.to_le_bytes()].concat();
}

#[test]
fn every_request_type_can_be_answered_ok() {
    let answers: [(Answer, u8); 8] = [
        (Answer::Arm(Outcome::Ok), 0x20),
        (Answer::Disarm(Outcome::Ok), 0x21),
        (Answer::TakeOff(Outcome::Ok), 0x22),
        (Answer::Land(Outcome::Ok), 0x23),
        (Answer::GoHome(Outcome::Ok), 0x24),
        (Answer::Flip(Outcome::Ok), 0x25),
        (Answer::SteadyState(Outcome::Ok), 0x26),
        (Answer::ExitSteadyState(Outcome::Ok), 0x27),
    ];
    for (answer, answers) in answers {
        check_reply(answer, &payload(answers, 0, 0));
    }
}

#[test]
fn every_refusal_with_its_reason() {
    fn arm(r: ArmRefusal) -> Answer {
        return Answer::Arm(Outcome::Refused(r));
    }
    fn take_off(r: TakeOffRefusal) -> Answer {
        return Answer::TakeOff(Outcome::Refused(r));
    }
    fn flip(r: FlipRefusal) -> Answer {
        return Answer::Flip(Outcome::Refused(r));
    }
    let cases = [
        (arm(ArmRefusal::BatteryLow), 0x20, 4),
        (arm(ArmRefusal::ThrottleNotLow), 0x20, 5),
        (arm(ArmRefusal::ImuNotReady), 0x20, 6),
        (arm(ArmRefusal::ArmSwitchOff), 0x20, 7),
        (take_off(TakeOffRefusal::NotArmed), 0x22, 1),
        (take_off(TakeOffRefusal::BatteryLow), 0x22, 4),
        (take_off(TakeOffRefusal::TooLate), 0x22, 14),
        (Answer::Land(Outcome::Refused(LandRefusal::NotFlying)), 0x23, 2),
        (flip(FlipRefusal::NotArmed), 0x25, 1),
        (flip(FlipRefusal::NotFlying), 0x25, 2),
        (flip(FlipRefusal::TooLow), 0x25, 3),
        (flip(FlipRefusal::TooLate), 0x25, 14),
        (
            Answer::SteadyState(Outcome::Refused(SteadyStateRefusal::NotFlying)),
            0x26,
            2,
        ),
    ];
    for (answer, answers, reason) in cases {
        check_reply(answer, &payload(answers, 1, reason));
    }
}

#[test]
fn a_reason_has_one_number_for_every_request() {
    // protocol.md §5.4: one table of reasons; each request may use only some of them.
    assert_eq!(Reason::from(TakeOffRefusal::NotArmed), Reason::NotArmed);
    assert_eq!(Reason::from(FlipRefusal::NotArmed), Reason::NotArmed);
    assert_eq!(
        Reason::from(ArmRefusal::BatteryLow),
        Reason::from(TakeOffRefusal::BatteryLow)
    );
    assert_eq!(Reason::from(ArmRefusal::ArmSwitchOff), Reason::ArmSwitchOff);
    assert_eq!(
        Reason::from(FlipRefusal::TooLate),
        Reason::from(TakeOffRefusal::TooLate)
    );
    assert_eq!(
        Reason::from(LandRefusal::NotFlying),
        Reason::from(SteadyStateRefusal::NotFlying)
    );
    check_reply(
        Answer::TakeOff(Outcome::Refused(TakeOffRefusal::NotArmed)),
        &payload(0x22, 1, 1),
    );
    check_reply(
        Answer::Flip(Outcome::Refused(FlipRefusal::NotArmed)),
        &payload(0x25, 1, 1),
    );
}

#[test]
fn failures_without_a_reason() {
    let cases: [(Outcome<Infallible>, u8); 3] = [
        (Outcome::NotSupported, 4),
        (Outcome::Busy, 5),
        (Outcome::NotAllowedNow, 6), // e.g. in steady state
    ];
    for (failure, status) in cases {
        check_reply(Answer::Disarm(failure), &payload(0x21, status, 0));
        check_reply(Answer::GoHome(failure), &payload(0x24, status, 0));
        check_reply(Answer::ExitSteadyState(failure), &payload(0x27, status, 0));
    }
}

#[test]
fn reply_to_takes_request_id_and_type_from_the_request() {
    let flip = Flip {
        request_id: RequestId(9),
        sent_at: 0,
        direction: 0,
        duration_ms: 300,
    };
    let refused = Outcome::Refused(FlipRefusal::TooLow);
    assert_eq!(
        parts(Some(Reply::to(&flip, refused))),
        Some((RequestId(9), Answer::Flip(refused)))
    );
    let arm = Arm {
        request_id: RequestId(10),
        sent_at: 0,
    };
    assert_eq!(Reply::to(&arm, Outcome::Ok).answer(), Answer::Arm(Outcome::Ok));
}

#[test]
fn malformed() {
    check_reply(Answer::Malformed(RequestType::Arm), &payload(0x20, 3, 0));
    check_reply(Answer::Malformed(RequestType::Flip), &payload(0x25, 3, 0));
}

/// A packet with this type and payload, as `Packet::parse` gives it.
fn reply_to_invalid(message_type: u8, sequence: u16, payload: &[u8]) -> Option<Reply> {
    let bytes = packet(0, message_type, sequence, payload);
    return Reply::to_invalid(&Packet::parse(&bytes).unwrap());
}

#[test]
fn reply_to_a_malformed_request() {
    let malformed = |id, request_type| return Some((RequestId(id), Answer::Malformed(request_type)));
    assert_eq!(
        parts(reply_to_invalid(0x20, 1, &[5, 0, 9])),
        malformed(5, RequestType::Arm)
    );
    assert_eq!(
        parts(reply_to_invalid(0x25, 1, &[6, 1, 0])),
        malformed(0x0106, RequestType::Flip)
    );
    assert_eq!(
        parts(reply_to_invalid(0x25, 1, &[6, 1])),
        malformed(0x0106, RequestType::Flip)
    );
}

#[test]
fn reply_to_an_unknown_type() {
    let expected = (RequestId(7), Answer::UnknownType(UnknownType::new(0xEE).unwrap()));
    assert_eq!(parts(reply_to_invalid(0xEE, 1, &[7, 0])), Some(expected));
}

#[test]
fn no_reply_when_there_is_nothing_to_answer() {
    let cases: [(&str, u8, u16, &[u8]); 6] = [
        ("a valid request", 0x20, 1, &[5, 0, 1, 2, 3, 4]),
        ("too short for a Request ID", 0x20, 1, &[5]),
        ("unknown type too short for a Request ID", 0xEE, 1, &[]),
        ("not a request", 0x10, 1, &[0; 3]),
        ("Hello, not a request", 0x01, 7, &[1, 0, 0, 0, 0]),
        ("Reply, not a request", 0x40, 1, &[1, 0, 0x20]),
    ];
    for (case, message_type, sequence, payload) in cases {
        assert_eq!(reply_to_invalid(message_type, sequence, payload), None, "{case}");
    }
}

#[test]
fn unknown_type() {
    let answer = Answer::UnknownType(UnknownType::new(0xEE).unwrap());
    check_reply(answer, &payload(0xEE, 2, 0));
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
    let land = Land {
        request_id: RequestId(0),
        sent_at: 0,
    };
    check_round_trip(
        Message::Reply(Reply::to(&land, Outcome::Ok)),
        REPLY,
        &[0, 0, 0x23, 0, 0, 0],
    );
}

#[test]
fn rejects_unknown_codes() {
    // The protocol version is bumped when codes are added, so after Hello an unknown code is an error.
    let unknown_reason = MessageError::UnknownCode {
        kind: "Reason",
        value: 0x0999,
    };
    assert_eq!(decode_payload(REPLY, &payload(0x20, 1, 0x0999)), Err(unknown_reason));
    assert_eq!(decode_payload(REPLY, &payload(0x21, 1, 0x0999)), Err(unknown_reason)); // Disarm
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
        ("Malformed for a type that is not a request", payload(0x10, 3, 0)),
        ("Malformed for an unknown type", payload(0xEE, 3, 0)),
        ("Not supported with a reason", payload(0x20, 4, 1)),
        ("Not allowed now with a reason", payload(0x25, 6, 1)),
        ("Disarm refused", payload(0x21, 1, 1)),
        ("Flip refused because the throttle is not low", payload(0x25, 1, 5)),
        ("Arm refused because it is not armed", payload(0x20, 1, 1)),
        (
            "Arm refused as too late: only TakeOff and Flip expire",
            payload(0x20, 1, 14),
        ),
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
