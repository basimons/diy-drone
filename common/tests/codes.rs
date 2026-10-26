use core::convert::Infallible;
use core::num::NonZeroU16;
use drone_common::wire::codes::*;
use std::fmt::Debug;

/// Both directions for every listed code; every other byte is `Unknown` and encodes back unchanged.
fn check<T>(table: &[(T, u8)], unknown: fn(Unknown<T>) -> T)
where
    T: Code<Repr = u8> + From<u8> + Into<u8> + PartialEq + Debug,
{
    for &(code, byte) in table {
        assert_eq!(code.into(), byte, "{code:?} encodes wrong");
        assert_eq!(T::from(byte), code, "byte {byte:#04x} decodes wrong");
        assert_eq!(Unknown::<T>::new(byte), None, "{code:?} is known");
    }
    for byte in 0..=255u8 {
        if !table.iter().any(|&(_, b)| return b == byte) {
            let expected = unknown(Unknown::new(byte).unwrap());
            assert_eq!(T::from(byte), expected, "byte {byte:#04x} should be unknown");
            assert_eq!(expected.into(), byte, "unknown {byte:#04x} encodes wrong");
        }
    }
}

/// Two-byte codes: 0 means "none" on the wire, so it is never a code, not even an unknown one.
fn check_u16<T>(table: &[(T, u16)], unknown: fn(Unknown<T>) -> T)
where
    T: Code<Repr = NonZeroU16> + From<NonZeroU16> + Into<NonZeroU16> + PartialEq + Debug,
{
    for &(code, value) in table {
        let value = NonZeroU16::new(value).expect("0 means none");
        assert_eq!(code.into(), value, "{code:?} encodes wrong");
        assert_eq!(T::from(value), code, "{value:#06x} decodes wrong");
        assert_eq!(Unknown::<T>::new(value), None, "{code:?} is known");
    }
    for value in 1..=u16::MAX {
        if !table.iter().any(|&(_, v)| return v == value) {
            let value = NonZeroU16::new(value).unwrap();
            let expected = unknown(Unknown::new(value).unwrap());
            assert_eq!(T::from(value), expected, "{value:#06x} should be unknown");
            assert_eq!(expected.into(), value, "unknown {value:#06x} encodes wrong");
        }
    }
}

#[test]
fn message_types_match_protocol_table() {
    use MessageType::*;
    check(
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
            (StreamUpdate, 0x63),
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
    check(&[(Ok, 0), (VersionMismatch, 1), (OptionNotSupported, 2)], Unknown);
}

#[test]
fn link_test_directions() {
    use LinkTestDirection::*;
    check(&[(DroneSends, 0), (ClientSends, 1)], Unknown);
}

#[test]
fn reply_statuses() {
    use Status::*;
    check(
        &[
            (Ok, 0),
            (Refused, 1),
            (UnknownType, 2),
            (Malformed, 3),
            (NotSupported, 4),
            (Busy, 5),
        ],
        Unknown,
    );
}

#[test]
fn event_levels() {
    use EventLevel::*;
    check(&[(Debug, 0), (Info, 1), (Warning, 2), (Error, 3)], Unknown);
}

#[test]
fn stream_ids() {
    use StreamId::*;
    check(
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

#[test]
fn event_codes() {
    use EventCode::*;
    check_u16(
        &[
            (Armed, 0x0101),
            (Disarmed, 0x0102),
            (LinkLost, 0x0201),
            (LinkRestored, 0x0202),
            (BatteryLow, 0x0301),
            (BatteryCritical, 0x0302),
            (FailsafeTriggered, 0x0401),
            (LoopOverrun, 0x0402),
            (ImuError, 0x0403),
        ],
        Unknown,
    );
}

#[test]
fn refusal_codes_are_numbered_per_request() {
    check_u16(
        &[
            (ArmRefusal::ThrottleNotLow, 1),
            (ArmRefusal::ImuNotReady, 2),
            (ArmRefusal::BatteryLow, 3),
        ],
        ArmRefusal::Unknown,
    );
    check_u16(
        &[(TakeOffRefusal::NotArmed, 1), (TakeOffRefusal::BatteryLow, 2)],
        TakeOffRefusal::Unknown,
    );
    check_u16(&[(LandRefusal::NotFlying, 1)], LandRefusal::Unknown);
    check_u16(
        &[
            (FlipRefusal::NotArmed, 1),
            (FlipRefusal::NotFlying, 2),
            (FlipRefusal::TooLow, 3),
        ],
        FlipRefusal::Unknown,
    );
    check_u16(&[(SteadyStateRefusal::NotFlying, 1)], SteadyStateRefusal::Unknown);
    check_u16(&[(LinkTestRefusal::NotAllowedNow, 1)], LinkTestRefusal::Unknown);
    check_u16(
        &[
            (SetSettingsRefusal::UnknownGroup, 1),
            (SetSettingsRefusal::ValueOutOfRange, 2),
        ],
        SetSettingsRefusal::Unknown,
    );
    check_u16(&[(GetSettingsRefusal::UnknownGroup, 1)], GetSettingsRefusal::Unknown);
    check_u16(&[(RequestOnceRefusal::UnknownStream, 1)], RequestOnceRefusal::Unknown);
    check_u16(
        &[
            (SubscribeRefusal::UnknownStream, 1),
            (SubscribeRefusal::RateNotPossible, 2),
        ],
        SubscribeRefusal::Unknown,
    );
    check_u16(
        &[(ReadLogRefusal::UnknownLog, 1), (ReadLogRefusal::OffsetPastEnd, 2)],
        ReadLogRefusal::Unknown,
    );
}

#[test]
fn a_refusal_always_has_a_reason() {
    // On the wire, Reason 0 means "no reason", which a refusal can't have.
    assert_eq!(ArmRefusal::from_reason(0), None);
    assert_eq!(ArmRefusal::from_reason(2), Some(ArmRefusal::ImuNotReady));
    assert_eq!(ArmRefusal::ImuNotReady.reason(), 2);
    let unknown = ArmRefusal::from_reason(0x0999).unwrap();
    assert_eq!(unknown.reason(), 0x0999);
}

#[test]
fn some_requests_are_never_refused() {
    for reason in 0..=u16::MAX {
        assert_eq!(Infallible::from_reason(reason), None);
    }
}

#[test]
fn unknown_never_holds_a_known_value() {
    assert_eq!(Unknown::<MessageType>::new(0x20), None); // Arm
    assert_eq!(Unknown::<MessageType>::new(0xEE).map(|u| return u.value()), Some(0xEE));
    assert_eq!(Unknown::<Status>::new(0xEE).map(|u| return u.value()), Some(0xEE));
    assert_eq!(Unknown::<Status>::new(1), None);
    assert_eq!(format!("{:?}", Unknown::<Status>::new(9).unwrap()), "Unknown(9)");
}
