mod support;

use drone_common::domain::Sticks;
use drone_common::wire::message::Message;
use support::*;

const STICKS: u8 = 0x10;

#[test]
fn sticks() {
    let sticks = Message::Sticks(Sticks {
        throttle: 0xABCD,
        roll: -2,
        pitch: 0x1234,
        yaw: -32768,
    });
    check_round_trip(sticks, STICKS, &[0xCD, 0xAB, 0xFE, 0xFF, 0x34, 0x12, 0x00, 0x80]);
}

#[test]
fn sticks_extremes() {
    let low = Sticks {
        throttle: 0,
        roll: i16::MIN,
        pitch: i16::MIN,
        yaw: i16::MIN,
    };
    let high = Sticks {
        throttle: u16::MAX,
        roll: i16::MAX,
        pitch: i16::MAX,
        yaw: i16::MAX,
    };
    check_round_trip(Message::Sticks(low), STICKS, &[0, 0, 0, 0x80, 0, 0x80, 0, 0x80]);
    check_round_trip(
        Message::Sticks(high),
        STICKS,
        &[0xFF, 0xFF, 0xFF, 0x7F, 0xFF, 0x7F, 0xFF, 0x7F],
    );
}

#[test]
fn sticks_rejects_wrong_length() {
    check_only_length(STICKS, 8, 0);
}
