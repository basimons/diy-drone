mod support;

use drone_common::domain::{Command, Flip};
use drone_common::wire::message::{CommandRequest, Message, MessageError, RequestId};
use support::*;

const FLIP: u8 = 0x25;

/// Every command whose payload is only the Request ID, with its type byte.
const SIMPLE: [(Command, u8); 7] = [
    (Command::Arm, 0x20),
    (Command::Disarm, 0x21),
    (Command::TakeOff, 0x22),
    (Command::Land, 0x23),
    (Command::GoHome, 0x24),
    (Command::SteadyState, 0x26),
    (Command::ExitSteadyState, 0x27),
];

fn request(request_id: u16, command: Command) -> Message {
    return Message::Command(CommandRequest {
        request_id: RequestId::new(request_id).unwrap(),
        command,
    });
}

#[test]
fn request_id_is_never_0() {
    assert_eq!(RequestId::new(0), None);
    assert_eq!(RequestId::new(1).map(|id| return id.get()), Some(1));
    assert_eq!(RequestId::new(u16::MAX).map(|id| return id.get()), Some(u16::MAX));
}

#[test]
fn commands_with_only_a_request_id() {
    for (command, message_type) in SIMPLE {
        check_round_trip(request(0x1234, command), message_type, &[0x34, 0x12]);
    }
}

#[test]
fn flip() {
    let flip = Command::Flip(Flip {
        direction: 0xC000, // 270°: to the left
        duration_ms: 300,
    });
    check_round_trip(request(7, flip), FLIP, &[0x07, 0x00, 0x00, 0xC0, 0x2C, 0x01]);
}

#[test]
fn commands_reject_request_id_0() {
    // protocol.md §5.0: Request ID 0 means "not a request".
    for (_, message_type) in SIMPLE {
        assert_eq!(decode_payload(message_type, &[0, 0]), Err(MessageError::InvalidValue));
    }
    assert_eq!(
        decode_payload(FLIP, &[0, 0, 0, 0, 0, 0]),
        Err(MessageError::InvalidValue)
    );
}

#[test]
fn commands_reject_wrong_length() {
    for (_, message_type) in SIMPLE {
        check_only_length(message_type, 2, 1);
    }
    check_only_length(FLIP, 6, 1);
}
