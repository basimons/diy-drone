mod support;

use drone_common::domain::{Command, Flip};
use drone_common::wire::message::{CommandRequest, Message, RequestId};
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
        request_id: RequestId(request_id),
        command,
    });
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
fn every_request_id_is_valid() {
    // protocol.md §5.0: the client's counter simply wraps, 0 included.
    for request_id in [0, 1, 0x8000, u16::MAX] {
        let bytes = request_id.to_le_bytes();
        check_round_trip(request(request_id, Command::Arm), 0x20, &bytes);
        let flip = Command::Flip(Flip {
            direction: 0,
            duration_ms: 0,
        });
        check_round_trip(request(request_id, flip), FLIP, &[bytes[0], bytes[1], 0, 0, 0, 0]);
    }
}

#[test]
fn commands_reject_wrong_length() {
    for (_, message_type) in SIMPLE {
        check_only_length(message_type, &[1, 0]);
    }
    check_only_length(FLIP, &[1, 0, 0, 0, 0, 0]);
}
