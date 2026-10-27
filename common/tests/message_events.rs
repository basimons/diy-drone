mod support;

use drone_common::wire::event::*;
use drone_common::wire::message::{FlightMode, Message, MessageError};
use support::*;

const EVENT: u8 = 0x41;

/// Checks `kind` as Event ID 0x0102: its payload is that ID, then `payload`.
fn check(kind: EventKind, payload: &[u8]) {
    let event = Event {
        event_id: EventId(0x0102),
        kind,
    };
    check_round_trip(Message::Event(event), EVENT, &with_id(payload));
}

/// Event ID 0x0102, then `rest`.
fn with_id(rest: &[u8]) -> Vec<u8> {
    return [&[0x02, 0x01][..], rest].concat();
}

#[test]
fn events_without_data() {
    check(EventKind::Armed(Armed), &[1, 0x01, 0x01]);
    check(EventKind::Disarmed(Disarmed), &[1, 0x02, 0x01]);
    check(EventKind::LinkLost(LinkLost), &[2, 0x01, 0x02]);
    check(EventKind::LinkRestored(LinkRestored), &[1, 0x02, 0x02]);
    check(EventKind::ImuError(ImuError), &[3, 0x03, 0x04]);
}

#[test]
fn events_with_data() {
    check(
        EventKind::BatteryLow(BatteryLow { millivolts: 14_000 }),
        &[2, 0x01, 0x03, 0xB0, 0x36],
    );
    check(
        EventKind::BatteryCritical(BatteryCritical { millivolts: 13_200 }),
        &[3, 0x02, 0x03, 0x90, 0x33],
    );
    check(
        EventKind::LoopOverrun(LoopOverrun { late_us: 0x0001_0203 }),
        &[3, 0x02, 0x04, 0x03, 0x02, 0x01, 0x00],
    );
}

#[test]
fn every_protection_cause() {
    use ProtectionCause::*;
    for (cause, byte) in [(LinkLoss, 1), (ClientFrozen, 2), (LowBattery, 3), (LoopOverrun, 4)] {
        check(
            EventKind::ProtectionTriggered(ProtectionTriggered { cause }),
            &[3, 0x01, 0x04, byte],
        );
    }
}

#[test]
fn every_flight_mode_change() {
    for (mode, byte) in [(FlightMode::Rate, 1), (FlightMode::Angle, 2)] {
        check(
            EventKind::FlightModeChanged(FlightModeChanged { mode }),
            &[1, 0x01, 0x05, byte],
        );
    }
}

#[test]
fn every_event_id_is_valid() {
    // protocol.md §5.4: the drone's counter simply wraps, 0 included.
    for id in [0, 1, 0x8000, u16::MAX] {
        let event = Event {
            event_id: EventId(id),
            kind: EventKind::Armed(Armed),
        };
        check_round_trip(
            Message::Event(event),
            EVENT,
            &[&id.to_le_bytes()[..], &[1, 0x01, 0x01]].concat(),
        );
    }
}

#[test]
fn each_event_kind_has_a_fixed_code_and_level() {
    assert_eq!((Armed::CODE, Armed::LEVEL), (0x0101, EventLevel::Info));
    assert_eq!((BatteryLow::CODE, BatteryLow::LEVEL), (0x0301, EventLevel::Warning));
    assert_eq!(
        EventKind::BatteryCritical(BatteryCritical { millivolts: 1 }).level(),
        EventLevel::Error
    );
}

#[test]
fn text_at_every_level() {
    let text = "batterie faible: 3,4 V ⚡";
    for (level, byte) in [
        (EventLevel::Debug, 0),
        (EventLevel::Info, 1),
        (EventLevel::Warning, 2),
        (EventLevel::Error, 3),
    ] {
        let event = EventKind::Text(Text {
            level,
            text: EventText::new(text).unwrap(),
        });
        assert_eq!(event.level(), level);
        check(event, &[[byte, 0, 0].as_slice(), text.as_bytes()].concat());
    }
}

#[test]
fn text_is_never_empty_and_at_most_244_bytes() {
    // 249 bytes of payload, minus Event ID, Level and Code.
    assert_eq!(MAX_EVENT_TEXT_LEN, 244);
    let longest = "x".repeat(MAX_EVENT_TEXT_LEN);
    let event = EventKind::Text(Text {
        level: EventLevel::Info,
        text: EventText::new(&longest).unwrap(),
    });
    check(event, &[[1, 0, 0].as_slice(), longest.as_bytes()].concat());
    assert_eq!(EventText::new(&"x".repeat(MAX_EVENT_TEXT_LEN + 1)), None);
    assert_eq!(EventText::new(""), None);
}

#[test]
fn event_text_is_used_like_a_str() {
    let text = EventText::new("battery low").unwrap();
    assert!(text.starts_with("battery"));
    assert_eq!(text.len(), 11);
    assert_eq!(&*text, "battery low");
    assert_eq!(text.to_string(), "battery low");
    assert_eq!(format!("[{:>5}]", EventText::new("ab").unwrap()), "[   ab]");
}

#[test]
fn event_text_is_formatted_without_a_heap() {
    let cell = 3;
    let text = EventText::format(format_args!("cell {cell} at {:.1} V", 3.35)).unwrap();
    assert_eq!(&*text, "cell 3 at 3.4 V");
    let too_long = "x".repeat(MAX_EVENT_TEXT_LEN + 1);
    assert_eq!(EventText::format(format_args!("{too_long}")), None);
    assert_eq!(EventText::format(format_args!("")), None);
    let exactly_max = "x".repeat(MAX_EVENT_TEXT_LEN);
    assert_eq!(
        EventText::format(format_args!("{exactly_max}")).map(|t| return t.len()),
        Some(MAX_EVENT_TEXT_LEN)
    );
    // The first part fits, the second doesn't: no half-written text comes out.
    let (first, second) = ("x".repeat(240), "y".repeat(10));
    assert_eq!(EventText::format(format_args!("{first}{second}")), None);
}

#[test]
fn event_texts_compare_and_print_as_text() {
    assert_eq!(EventText::new("abc"), EventText::new("abc"));
    assert_ne!(EventText::new("abc"), EventText::new("abd"));
    assert_eq!(format!("{:?}", EventText::new("abc").unwrap()), "\"abc\"");
}

#[test]
fn rejects_unknown_codes() {
    // The protocol version is bumped when codes are added, so after Hello an unknown code is an error.
    let unknown = |kind, value| return Err(MessageError::UnknownCode { kind, value });
    assert_eq!(
        decode_payload(EVENT, &with_id(&[2, 0x99, 0x09, 1, 2])),
        unknown("EventCode", 0x0999)
    );
    assert_eq!(
        decode_payload(EVENT, &with_id(&[9, 0, 0, b'h', b'i'])),
        unknown("EventLevel", 9)
    );
    assert_eq!(
        decode_payload(EVENT, &with_id(&[9, 0x01, 0x01])),
        unknown("EventLevel", 9)
    );
    assert_eq!(
        decode_payload(EVENT, &with_id(&[3, 0x01, 0x04, 0])),
        unknown("ProtectionCause", 0)
    );
    assert_eq!(
        decode_payload(EVENT, &with_id(&[3, 0x01, 0x04, 0xEE])),
        unknown("ProtectionCause", 0xEE)
    );
    assert_eq!(
        decode_payload(EVENT, &with_id(&[1, 0x01, 0x05, 0])),
        unknown("FlightMode", 0)
    );
}

#[test]
fn rejects_known_events_that_dont_fit() {
    use MessageError::{InvalidValue, WrongLength};
    let cases: [(&str, Vec<u8>, MessageError); 11] = [
        ("Armed with level Error", vec![3, 0x01, 0x01], InvalidValue),
        ("Armed with data", vec![1, 0x01, 0x01, 0], WrongLength),
        ("BatteryLow without its voltage", vec![2, 0x01, 0x03], WrongLength),
        ("BatteryLow with half its voltage", vec![2, 0x01, 0x03, 0], WrongLength),
        (
            "BatteryLow with too much data",
            vec![2, 0x01, 0x03, 0, 0, 0],
            WrongLength,
        ),
        ("ProtectionTriggered without a cause", vec![3, 0x01, 0x04], WrongLength),
        (
            "ProtectionTriggered with too much data",
            vec![3, 0x01, 0x04, 1, 0],
            WrongLength,
        ),
        ("LoopOverrun with 2 of 4 bytes", vec![3, 0x02, 0x04, 0, 0], WrongLength),
        ("Text without text", vec![1, 0, 0], InvalidValue),
        ("invalid UTF-8 text", vec![1, 0, 0, 0xFF], InvalidValue),
        ("cut-off UTF-8 character", vec![1, 0, 0, 0xE2, 0x9A], InvalidValue),
    ];
    for (case, payload, error) in cases {
        assert_eq!(decode_payload(EVENT, &with_id(&payload)), Err(error), "{case}");
    }
}

#[test]
fn rejects_too_short() {
    for length in 0..5 {
        assert_eq!(decode_payload(EVENT, &vec![1; length]), Err(MessageError::WrongLength));
    }
}

#[test]
fn every_event_header_decodes_or_is_rejected() {
    // Every Level × Code with a few data lengths: decoding never panics, and anything accepted encodes back.
    for level in 0..=255u8 {
        let around_codes = (0..=4u16).flat_map(|group| return (0..=5).map(move |n| return group << 8 | n));
        for code in around_codes.chain([0x0999, u16::MAX]) {
            for data in [&[][..], &[1][..], &[1, 2][..], &[1, 2, 3, 4][..]] {
                let payload = [&[0x02, 0x01, level][..], &code.to_le_bytes(), data].concat();
                if let Ok(message) = decode_payload(EVENT, &payload) {
                    assert_eq!(encode_packet(&message, 0), packet(0, EVENT, 0, &payload));
                }
            }
        }
    }
}

#[test]
fn event_text_is_no_bigger_than_its_bytes_and_length() {
    // Every Message is as big as the biggest one (protocol.md §7.1), and EventText is in the biggest.
    assert_eq!(size_of::<EventText>(), MAX_EVENT_TEXT_LEN + 1);
}
