use drone_common::wire::codes::*;
use std::fmt::Debug;

fn assert_all_matches_table<T: Copy + PartialEq + Debug>(all: &[T], table: &[(T, u8)]) {
    let listed: Vec<T> = table.iter().map(|&(code, _)| return code).collect();
    assert_eq!(all, &listed[..], "ALL must list every code, in protocol order");
}

/// Checks both directions for every listed code, and that every other byte is rejected.
fn check<T>(all: &[T], table: &[(T, u8)])
where
    T: TryFrom<u8, Error = UnknownCode> + Into<u8> + Copy + PartialEq + Debug,
{
    assert_all_matches_table(all, table);
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
fn check_open<T>(all: &[T], table: &[(T, u8)], unknown: fn(u8) -> T)
where
    T: From<u8> + Into<u8> + Copy + PartialEq + Debug,
{
    assert_all_matches_table(all, table);
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
        &MessageType::ALL,
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
    check(
        &HelloResult::ALL,
        &[(Ok, 0), (VersionMismatch, 1), (OptionNotSupported, 2)],
    );
}

#[test]
fn link_test_directions() {
    use LinkTestDirection::*;
    check(&LinkTestDirection::ALL, &[(DroneSends, 0), (ClientSends, 1)]);
}

#[test]
fn reply_statuses() {
    use Status::*;
    check(
        &Status::ALL,
        &[
            (Ok, 0),
            (Refused, 1),
            (UnknownType, 2),
            (Malformed, 3),
            (NotSupported, 4),
            (Busy, 5),
        ],
    );
}

#[test]
fn event_levels() {
    use EventLevel::*;
    check(&EventLevel::ALL, &[(Debug, 0), (Info, 1), (Warning, 2), (Error, 3)]);
}

#[test]
fn stream_ids() {
    use StreamId::*;
    check_open(
        &StreamId::ALL,
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
