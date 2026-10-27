mod support;

use drone_common::wire::event::{Armed, Event, EventId, EventKind};
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
fn expected(command: Command) -> (u8, Message, Vec<u8>) {
    let request_id = RequestId(7);
    let sent_at = 0x0403_0201;
    let header = vec![0x07, 0x00, 0x01, 0x02, 0x03, 0x04];
    return match command {
        Command::Arm => (0x20, Message::Arm(Arm { request_id, sent_at }), header),
        Command::Disarm => (0x21, Message::Disarm(Disarm { request_id, sent_at }), header),
        Command::TakeOff => (0x22, Message::TakeOff(TakeOff { request_id, sent_at }), header),
        Command::Land => (0x23, Message::Land(Land { request_id, sent_at }), header),
        Command::GoHome => (0x24, Message::GoHome(GoHome { request_id, sent_at }), header),
        Command::Flip { direction, duration_ms } => {
            let flip = Flip {
                request_id,
                sent_at,
                direction,
                duration_ms,
            };
            let fields = [direction.to_le_bytes(), duration_ms.to_le_bytes()].concat();
            (FLIP, Message::Flip(flip), [header, fields].concat())
        }
        Command::SteadyState => (0x26, Message::SteadyState(SteadyState { request_id, sent_at }), header),
        Command::ExitSteadyState => (
            0x27,
            Message::ExitSteadyState(ExitSteadyState { request_id, sent_at }),
            header,
        ),
    };
}

fn request(request_id: u16, sent_at: u32, command: Command) -> CommandRequest {
    return CommandRequest {
        request_id: RequestId(request_id),
        sent_at,
        command,
    };
}

#[test]
fn every_command_starts_with_request_id_and_sent_at() {
    for command in ALL {
        let (message_type, message, payload) = expected(command);
        check_round_trip(message, message_type, &payload);
    }
}

#[test]
fn flip_message() {
    let (_, message, payload) = expected(FLIP_LEFT);
    assert_eq!(payload, [0x07, 0x00, 0x01, 0x02, 0x03, 0x04, 0x00, 0xC0, 0x2C, 0x01]);
    check_round_trip(message, FLIP, &payload);
}

#[test]
fn a_command_request_becomes_its_message_and_back() {
    for command in ALL {
        let (_, message, _) = expected(command);
        let request = request(7, 0x0403_0201, command);
        assert_eq!(Message::from(request), message);
        assert_eq!(message.command(), Some(request));
    }
}

#[test]
fn request_id_and_sent_at_are_carried_unchanged() {
    for (request_id, sent_at) in [(0u16, 0u32), (1, 2), (0xFFFF, u32::MAX), (0x8000, 0x8000_0000)] {
        for command in ALL {
            let (message_type, _, payload) = expected(command);
            let payload = [&request_id.to_le_bytes()[..], &sent_at.to_le_bytes(), &payload[6..]].concat();
            let request = request(request_id, sent_at, command);
            check_round_trip(Message::from(request), message_type, &payload);
            assert_eq!(
                decode_payload(message_type, &payload).map(|m| return m.command()),
                Ok(Some(request))
            );
        }
    }
}

#[test]
fn other_messages_are_no_command() {
    let others = [
        Message::Ping(Ping { timestamp: 1 }),
        Message::Sticks(Sticks {
            throttle: 0,
            roll: 0,
            pitch: 0,
            yaw: 0,
        }),
        Message::Ack(Ack {
            largest_received: 0,
            ack_delay_us: 0,
            received_before: 0,
        }),
        Message::Reply(Reply {
            request_id: RequestId(1),
            answer: Answer::Arm(Outcome::Ok),
        }),
        Message::Event(Event {
            event_id: EventId(1),
            kind: EventKind::Armed(Armed),
        }),
    ];
    for message in others {
        assert_eq!(message.command(), None);
    }
}

#[test]
fn all_lists_every_command_the_library_knows() {
    let decodes_to_a_command = |message_type: u8| {
        return [6, 10].into_iter().any(|len| {
            return decode_payload(message_type, &vec![0; len]).is_ok_and(|m| return m.command().is_some());
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
fn take_off_and_flip_expire_after_300_ms() {
    assert_eq!(COMMAND_EXPIRY_US, 300_000); // protocol.md §5.3
}

#[test]
fn requests_know_their_request_id() {
    let Message::Flip(flip) = Message::from(request(6, 0, FLIP_LEFT)) else {
        panic!("not a Flip");
    };
    assert_eq!(flip.request_id(), RequestId(6));
}

#[test]
fn commands_reject_wrong_length() {
    for command in ALL {
        let (message_type, _, payload) = expected(command);
        check_only_length(message_type, &payload);
    }
}
