mod support;

use drone_common::wire::codes::{EventCode, EventLevel, ReasonCode, RequestType, Status, UnknownType};
use drone_common::wire::message::{Event, EventText, MAX_EVENT_TEXT_LEN, Message, MessageError, Reply, RequestId};
use support::*;

const REPLY: u8 = 0x40;
const EVENT: u8 = 0x41;

fn reply(answers: RequestType, status: Status, reason: Option<ReasonCode>) -> Message {
    return Message::Reply(Reply {
        request_id: RequestId(0x0102),
        answers,
        status,
        reason,
    });
}

#[test]
fn reply_every_status() {
    let statuses = [
        (Status::Ok, 0),
        (Status::Refused, 1),
        (Status::UnknownType, 2),
        (Status::Malformed, 3),
        (Status::NotSupported, 4),
        (Status::Busy, 5),
    ];
    for (status, byte) in statuses {
        let message = reply(RequestType::Flip, status, Some(ReasonCode::TooLow));
        check_round_trip(message, REPLY, &[0x02, 0x01, 0x25, byte, 0x03, 0x02]);
    }
}

#[test]
fn reply_to_an_unknown_type_keeps_its_byte() {
    let message = reply(
        RequestType::Unknown(UnknownType::from_byte(0xEE).unwrap()),
        Status::UnknownType,
        None,
    );
    check_round_trip(message, REPLY, &[0x02, 0x01, 0xEE, 2, 0, 0]);
}

#[test]
fn reply_without_a_reason() {
    // On the wire, "no reason" is 0; no ReasonCode has that value.
    check_round_trip(
        reply(RequestType::Arm, Status::Ok, None),
        REPLY,
        &[0x02, 0x01, 0x20, 0, 0, 0],
    );
}

#[test]
fn reply_rejects_bad_values() {
    let cases: [(&str, [u8; 6]); 5] = [
        ("unknown status", [1, 0, 0x20, 6, 0, 0]),
        ("unknown reason", [1, 0, 0x20, 1, 0xFF, 0xFF]),
        ("answers Sticks, not a request", [1, 0, 0x10, 0, 0, 0]),
        ("answers Hello, not a request", [1, 0, 0x01, 0, 0, 0]),
        ("answers a Reply, not a request", [1, 0, 0x40, 0, 0, 0]),
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
fn reply_with_request_id_0() {
    let message = Message::Reply(Reply {
        request_id: RequestId(0),
        answers: RequestType::Land,
        status: Status::Busy,
        reason: None,
    });
    check_round_trip(message, REPLY, &[0, 0, 0x23, 5, 0, 0]);
}

#[test]
fn reply_rejects_wrong_length() {
    check_only_length(REPLY, &[1, 0, 0x20, 0, 0, 0]);
}

fn event(level: EventLevel, code: Option<EventCode>, text: &str) -> Message {
    return Message::Event(Event {
        level,
        code,
        text: EventText::new(text).unwrap(),
    });
}

#[test]
fn event_every_level() {
    let levels = [
        (EventLevel::Debug, 0),
        (EventLevel::Info, 1),
        (EventLevel::Warning, 2),
        (EventLevel::Error, 3),
    ];
    for (level, byte) in levels {
        let message = event(level, Some(EventCode::BatteryCritical), "hi");
        check_round_trip(message, EVENT, &[byte, 0x02, 0x03, b'h', b'i']);
    }
}

#[test]
fn event_without_text() {
    check_round_trip(event(EventLevel::Info, None, ""), EVENT, &[1, 0, 0]);
}

#[test]
fn event_text_is_utf8() {
    let text = "batterie faible: 3,4 V ⚡";
    let payload = [&[2, 0x01, 0x03][..], text.as_bytes()].concat();
    check_round_trip(
        event(EventLevel::Warning, Some(EventCode::BatteryLow), text),
        EVENT,
        &payload,
    );
    let Ok(Message::Event(decoded)) = decode_payload(EVENT, &payload) else {
        panic!("not an event");
    };
    assert_eq!(decoded.text.as_str(), text);
}

#[test]
fn event_text_up_to_246_bytes() {
    // 249 bytes of payload, minus Level and Code.
    assert_eq!(MAX_EVENT_TEXT_LEN, 246);
    let longest = "x".repeat(MAX_EVENT_TEXT_LEN);
    let payload = [&[1, 0, 0][..], longest.as_bytes()].concat();
    check_round_trip(event(EventLevel::Info, None, &longest), EVENT, &payload);
    assert_eq!(EventText::new(&"x".repeat(MAX_EVENT_TEXT_LEN + 1)), None);
}

#[test]
fn event_texts_compare_and_print_as_text() {
    assert_eq!(EventText::new("abc"), EventText::new("abc"));
    assert_ne!(EventText::new("abc"), EventText::new("abd"));
    assert_eq!(EventText::default().as_str(), "");
    assert_eq!(format!("{:?}", EventText::new("abc").unwrap()), "\"abc\"");
}

#[test]
fn event_rejects_bad_values() {
    let cases: [(&str, Vec<u8>); 4] = [
        ("unknown level", vec![4, 0, 0]),
        ("unknown code", vec![1, 0xFF, 0xFF]),
        ("invalid UTF-8", vec![1, 0, 0, 0xFF]),
        ("cut-off UTF-8 character", vec![1, 0, 0, 0xE2, 0x9A]),
    ];
    for (case, payload) in cases {
        assert_eq!(
            decode_payload(EVENT, &payload),
            Err(MessageError::InvalidValue),
            "{case}"
        );
    }
}

#[test]
fn event_rejects_too_short() {
    for length in 0..3 {
        assert_eq!(decode_payload(EVENT, &vec![1; length]), Err(MessageError::WrongLength));
    }
}
