use drone_common::wire::codes::*;
use std::fmt::Debug;

/// Checks both directions for every listed code, and that every other byte is rejected.
fn check<T>(table: &[(T, u8)])
where
    T: TryFrom<u8, Error = UnknownCode> + Into<u8> + Copy + PartialEq + Debug,
{
    for &(code, byte) in table {
        assert_eq!(code.into(), byte, "{code:?} encodes wrong");
        assert_eq!(T::try_from(byte), Ok(code), "byte {byte:#04x} decodes wrong");
    }
    for byte in 0..=255u8 {
        if !table.iter().any(|&(_, b)| return b == byte) {
            assert_eq!(
                T::try_from(byte),
                Err(UnknownCode(byte)),
                "byte {byte:#04x} should be unknown"
            );
        }
    }
}

/// For enums with an `Unknown(byte)` fallback: every other byte decodes to `Unknown` and encodes back unchanged.
fn check_open<T>(table: &[(T, u8)], unknown: fn(u8) -> T)
where
    T: From<u8> + Into<u8> + Copy + PartialEq + Debug,
{
    for &(code, byte) in table {
        assert_eq!(code.into(), byte, "{code:?} encodes wrong");
        assert_eq!(T::from(byte), code, "byte {byte:#04x} decodes wrong");
    }
    for byte in 0..=255u8 {
        if !table.iter().any(|&(_, b)| return b == byte) {
            assert_eq!(T::from(byte), unknown(byte), "byte {byte:#04x} should be unknown");
            assert_eq!(unknown(byte).into(), byte, "Unknown({byte:#04x}) encodes wrong");
        }
    }
}

#[test]
fn message_types_match_protocol_table() {
    use MessageType::*;
    check_open(
        &[
            (Hello, 0x01),
            (HelloReply, 0x02),
            (Ping, 0x03),
            (Pong, 0x04),
            (LinkTest, 0x05),
            (LinkTestData, 0x06),
            (LinkTestResult, 0x07),
            (Sticks, 0x10),
            (Arm, 0x20),
            (Disarm, 0x21),
            (TakeOff, 0x22),
            (Land, 0x23),
            (GoHome, 0x24),
            (Flip, 0x25),
            (SteadyState, 0x26),
            (ExitSteadyState, 0x27),
            (Reply, 0x40),
            (Event, 0x41),
            (SetSettings, 0x50),
            (GetSettings, 0x51),
            (Settings, 0x52),
            (RequestOnce, 0x60),
            (Subscribe, 0x61),
            (StreamData, 0x62),
            (ListLogs, 0x70),
            (LogList, 0x71),
            (ReadLog, 0x72),
            (LogChunk, 0x73),
        ],
        Unknown,
    );
}

#[test]
fn hello_results() {
    use HelloResult::*;
    check(&[(Ok, 0), (VersionMismatch, 1), (OptionNotSupported, 2)]);
}

#[test]
fn link_test_directions() {
    use LinkTestDirection::*;
    check(&[(DroneSends, 0), (ClientSends, 1)]);
}

#[test]
fn reply_statuses() {
    use Status::*;
    check(&[
        (Ok, 0),
        (Refused, 1),
        (UnknownType, 2),
        (Malformed, 3),
        (NotSupported, 4),
        (Busy, 5),
    ]);
}

#[test]
fn event_levels() {
    use EventLevel::*;
    check(&[(Debug, 0), (Info, 1), (Warning, 2), (Error, 3)]);
}

#[test]
fn stream_ids() {
    use StreamId::*;
    check_open(
        &[
            (Status, 1),
            (Battery, 2),
            (Attitude, 3),
            (Height, 4),
            (RawImu, 5),
            (Gps, 6),
            (LinkQuality, 7),
        ],
        Unknown,
    );
}

/// protocol.md §5.0: every C→D message except Sticks, Hello and Ping.
fn requests() -> [(RequestType, u8); 16] {
    return [
        (RequestType::LinkTest, 0x05),
        (RequestType::Arm, 0x20),
        (RequestType::Disarm, 0x21),
        (RequestType::TakeOff, 0x22),
        (RequestType::Land, 0x23),
        (RequestType::GoHome, 0x24),
        (RequestType::Flip, 0x25),
        (RequestType::SteadyState, 0x26),
        (RequestType::ExitSteadyState, 0x27),
        (RequestType::SetSettings, 0x50),
        (RequestType::GetSettings, 0x51),
        (RequestType::RequestOnce, 0x60),
        (RequestType::Subscribe, 0x61),
        (RequestType::ListLogs, 0x70),
        (RequestType::ReadLog, 0x72),
        (RequestType::Unknown(UnknownType::from_byte(0xEE).unwrap()), 0xEE),
    ];
}

#[test]
fn request_types_are_message_types() {
    for (request, byte) in requests() {
        assert_eq!(u8::from(MessageType::from(request)), byte, "{request:?}");
        assert_eq!(
            RequestType::try_from(MessageType::from(byte)),
            Ok(request),
            "byte {byte:#04x}"
        );
    }
}

#[test]
fn other_known_types_are_not_requests() {
    for byte in 0..=255u8 {
        let message_type = MessageType::from(byte);
        let is_request = requests().iter().any(|&(_, b)| return b == byte);
        let is_unknown = matches!(message_type, MessageType::Unknown(_));
        if !is_request && !is_unknown {
            assert_eq!(RequestType::try_from(message_type), Err(NotARequest(message_type)));
        }
    }
}

#[test]
fn every_unknown_byte_is_an_unknown_request_type() {
    // The drone answers types it doesn't know with Reply "unknown type", so they must fit in a Reply.
    for byte in 0..=255u8 {
        if let MessageType::Unknown(_) = MessageType::from(byte) {
            let request = RequestType::try_from(MessageType::from(byte)).unwrap();
            assert_eq!(u8::from(MessageType::from(request)), byte);
        }
    }
}

#[test]
fn unknown_type_never_holds_a_known_byte() {
    assert_eq!(UnknownType::from_byte(0x20), None); // Arm
    assert_eq!(UnknownType::from_byte(0x10), None); // Sticks
    assert_eq!(UnknownType::from_byte(0xEE).map(|t| return t.byte()), Some(0xEE));
    // A hand-made `MessageType::Unknown` holding a known byte is read as the known type.
    assert_eq!(RequestType::try_from(MessageType::Unknown(0x20)), Ok(RequestType::Arm));
    assert_eq!(
        RequestType::try_from(MessageType::Unknown(0x10)),
        Err(NotARequest(MessageType::Sticks))
    );
}
