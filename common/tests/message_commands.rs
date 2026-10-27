mod support;

use core::convert::Infallible;
use drone_common::wire::message::*;
use drone_common::wire::reply::{Outcome, Reply};
use support::*;

const FLIP: u8 = 0x25;
const REPLY: u8 = 0x40;

const FLIP_LEFT: Command = Command::Flip {
    direction: 0xC000, // 270°: to the left
    duration_ms: 300,
};

const ALL: [Command; 8] = [
    Command::Arm,
    Command::Disarm,
    Command::TakeOff,
    Command::Land,
    Command::GoHome,
    FLIP_LEFT,
    Command::SteadyState,
    Command::ExitSteadyState,
];

/// The type byte, the message and the payload of `command` with Request ID 7 and Sent At 0x04030201. No `_` arm: a
/// new command doesn't compile until it is added here, and `all_lists_every_command_the_library_knows` fails until it
/// is in `ALL`.
fn expected(command: Command) -> (u8, CommandMessage, Vec<u8>) {
    let request_id = RequestId(7);
    let sent_at = 0x0403_0201;
    let header = vec![0x07, 0x00, 0x01, 0x02, 0x03, 0x04];
    return match command {
        Command::Arm => (0x20, CommandMessage::Arm(Arm { request_id, sent_at }), header),
        Command::Disarm => (0x21, CommandMessage::Disarm(Disarm { request_id, sent_at }), header),
        Command::TakeOff => (0x22, CommandMessage::TakeOff(TakeOff { request_id, sent_at }), header),
        Command::Land => (0x23, CommandMessage::Land(Land { request_id, sent_at }), header),
        Command::GoHome => (0x24, CommandMessage::GoHome(GoHome { request_id, sent_at }), header),
        Command::Flip { direction, duration_ms } => {
            let flip = Flip {
                request_id,
                sent_at,
                direction,
                duration_ms,
            };
            let fields = [direction.to_le_bytes(), duration_ms.to_le_bytes()].concat();
            (FLIP, CommandMessage::Flip(flip), [header, fields].concat())
        }
        Command::SteadyState => (
            0x26,
            CommandMessage::SteadyState(SteadyState { request_id, sent_at }),
            header,
        ),
        Command::ExitSteadyState => {
            let message = CommandMessage::ExitSteadyState(ExitSteadyState { request_id, sent_at });
            (0x27, message, header)
        }
    };
}

#[test]
fn every_command_starts_with_request_id_and_sent_at() {
    for command in ALL {
        let (message_type, message, payload) = expected(command);
        check_round_trip(Message::Command(message), message_type, &payload);
    }
}

#[test]
fn flip_message() {
    let (_, message, payload) = expected(FLIP_LEFT);
    assert_eq!(payload, [0x07, 0x00, 0x01, 0x02, 0x03, 0x04, 0x00, 0xC0, 0x2C, 0x01]);
    check_round_trip(Message::Command(message), FLIP, &payload);
}

#[test]
fn a_command_message_is_built_from_its_parts_and_gives_them_back() {
    for command in ALL {
        let message = CommandMessage::new(RequestId(7), 0x0403_0201, command);
        assert_eq!(message, expected(command).1);
        assert_eq!(message.request_id(), RequestId(7));
        assert_eq!(message.sent_at(), 0x0403_0201);
        assert_eq!(message.command(), command);
    }
}

#[test]
fn request_id_and_sent_at_are_carried_unchanged() {
    for (request_id, sent_at) in [(0u16, 0u32), (1, 2), (0xFFFF, u32::MAX), (0x8000, 0x8000_0000)] {
        for command in ALL {
            let (message_type, _, payload) = expected(command);
            let payload = [&request_id.to_le_bytes()[..], &sent_at.to_le_bytes(), &payload[6..]].concat();
            let message = CommandMessage::new(RequestId(request_id), sent_at, command);
            check_round_trip(Message::Command(message), message_type, &payload);
        }
    }
}

#[test]
fn all_lists_every_command_the_library_knows() {
    let decodes_to_a_command = |message_type: u8| {
        return [6, 10].into_iter().any(|len| {
            return matches!(decode_payload(message_type, &vec![0; len]), Ok(Message::Command(_)));
        });
    };
    let library: Vec<u8> = (0..=u8::MAX).filter(|&t| return decodes_to_a_command(t)).collect();
    let listed: Vec<u8> = ALL.into_iter().map(|command| return expected(command).0).collect();
    assert_eq!(library, listed);
}

#[test]
fn only_take_off_and_flip_expire() {
    let expiring: Vec<Command> = ALL.into_iter().filter(|command| return command.expires()).collect();
    assert_eq!(expiring, [Command::TakeOff, FLIP_LEFT]);
}

#[test]
fn exactly_the_expiring_commands_can_be_refused_too_late() {
    for command in ALL {
        let reply = [&[0x00, 0x00, expected(command).0, 1][..], &14u16.to_le_bytes()].concat(); // Refused, too late
        assert_eq!(decode_payload(REPLY, &reply).is_ok(), command.expires(), "{command:?}");
    }
}

#[test]
fn an_expiring_command_is_refused_too_late_by_its_own_reply() {
    for command in ALL {
        let (message_type, message, _) = expected(command);
        let reply = message.refuse_too_late();
        assert_eq!(reply.is_some(), command.expires(), "{command:?}");
        if let Some(reply) = reply {
            let bytes = [&[0x07, 0x00, message_type, 1][..], &14u16.to_le_bytes()].concat();
            check_round_trip(Message::Reply(reply), REPLY, &bytes);
        }
    }
}

#[test]
fn any_command_can_be_answered_without_a_refusal() {
    let cases: [(Outcome<Infallible>, u8); 4] = [
        (Outcome::Ok, 0),
        (Outcome::NotSupported, 4),
        (Outcome::Busy, 5),
        (Outcome::NotAllowedNow, 6),
    ];
    for command in ALL {
        let (message_type, message, _) = expected(command);
        for (outcome, status) in cases {
            let reply = message.reply(outcome);
            check_round_trip(Message::Reply(reply), REPLY, &[0x07, 0x00, message_type, status, 0, 0]);
        }
    }
}

#[test]
fn a_typed_reply_answers_the_command_it_was_built_from() {
    let CommandMessage::Flip(flip) = CommandMessage::new(RequestId(6), 0, FLIP_LEFT) else {
        panic!("not a Flip");
    };
    let reply = Reply::to(&flip, Outcome::Refused(FlipRefusal::TooLow));
    assert_eq!(reply.request_id(), RequestId(6));
    assert_eq!(reply.answer(), Answer::Flip(Outcome::Refused(FlipRefusal::TooLow)));
}

#[test]
fn take_off_and_flip_expire_after_300_ms() {
    assert_eq!(COMMAND_EXPIRY_US, 300_000); // protocol.md §5.3
}

#[test]
fn commands_reject_wrong_length() {
    for command in ALL {
        let (message_type, _, payload) = expected(command);
        check_only_length(message_type, &payload);
    }
}
