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

#[test]
fn message_types_match_protocol_table() {
    use MessageType::*;
    check(&[
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
    ]);
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
    check(&[
        (Status, 1),
        (Battery, 2),
        (Attitude, 3),
        (Height, 4),
        (RawImu, 5),
        (Gps, 6),
        (LinkQuality, 7),
    ]);
}
