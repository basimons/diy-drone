mod support;

use drone_common::wire::message::*;
use support::*;

const TAKE_OFF: u8 = 0x22;
const FLIP: u8 = 0x25;

/// Every command whose payload is only the Request ID, with its type byte.
fn simple(id: u16) -> [(Message, u8); 6] {
    let request_id = RequestId(id);
    return [
        (Message::Arm(Arm { request_id }), 0x20),
        (Message::Disarm(Disarm { request_id }), 0x21),
        (Message::Land(Land { request_id }), 0x23),
        (Message::GoHome(GoHome { request_id }), 0x24),
        (Message::SteadyState(SteadyState { request_id }), 0x26),
        (Message::ExitSteadyState(ExitSteadyState { request_id }), 0x27),
    ];
}

fn flip(request_id: u16, direction: u16, duration_ms: u16) -> Flip {
    return Flip {
        request_id: RequestId(request_id),
        sent_at: 0x0403_0201,
        direction,
        duration_ms,
    };
}

#[test]
fn commands_with_only_a_request_id() {
    for (message, message_type) in simple(0x1234) {
        check_round_trip(message, message_type, &[0x34, 0x12]);
    }
}

#[test]
fn take_off_message() {
    let take_off = TakeOff {
        request_id: RequestId(7),
        sent_at: 0x0403_0201,
    };
    check_round_trip(
        Message::TakeOff(take_off),
        TAKE_OFF,
        &[0x07, 0x00, 0x01, 0x02, 0x03, 0x04],
    );
}

#[test]
fn flip_message() {
    let flip = flip(7, 0xC000, 300); // 270°: to the left
    let payload = [0x07, 0x00, 0x01, 0x02, 0x03, 0x04, 0x00, 0xC0, 0x2C, 0x01];
    check_round_trip(Message::Flip(flip), FLIP, &payload);
}

#[test]
fn every_request_id_is_valid() {
    // protocol.md §5.0: the client's counter simply wraps, 0 included.
    for request_id in [0, 1, 0x8000, u16::MAX] {
        let bytes = request_id.to_le_bytes();
        check_round_trip(simple(request_id)[0].0, 0x20, &bytes);
        let flip = Message::Flip(flip(request_id, 0, 0));
        check_round_trip(flip, FLIP, &[bytes[0], bytes[1], 1, 2, 3, 4, 0, 0, 0, 0]);
    }
}

#[test]
fn requests_know_their_request_id() {
    assert_eq!(
        Arm {
            request_id: RequestId(5)
        }
        .request_id(),
        RequestId(5)
    );
    assert_eq!(flip(6, 0, 0).request_id(), RequestId(6));
}

#[test]
fn commands_reject_wrong_length() {
    for (_, message_type) in simple(0) {
        check_only_length(message_type, &[1, 0]);
    }
    check_only_length(TAKE_OFF, &[1, 0, 0, 0, 0, 0]);
    check_only_length(FLIP, &[1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
}
