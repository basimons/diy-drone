mod support;

use drone_common::wire::codes::HelloResult;
use drone_common::wire::frame::{FrameError, Received};
use drone_common::wire::message::*;
use support::*;

const HELLO: u8 = 0x01;
const HELLO_REPLY: u8 = 0x02;
const PING: u8 = 0x03;
const PONG: u8 = 0x04;
const MAX_PAYLOAD_LEN: usize = 249; // protocol.md §4.1

/// Decodes a hand-built frame with sequence 0.
fn decode(message_type: u8, payload: &[u8]) -> Result<Message, FrameError> {
    return read_one(&frame(0, message_type, 0, payload)).map(|r| return r.message);
}

/// Encodes `message`, checks it against a hand-built frame, and checks it decodes back to `message`.
fn check(message: Message, message_type: u8, payload: &[u8]) {
    let handshake = message_type == HELLO || message_type == HELLO_REPLY;
    let sequence = if handshake { 0 } else { 42 };
    let encoded = encode(&message, 42);
    assert_eq!(encoded, frame(0, message_type, sequence, payload));
    assert_eq!(read_one(&encoded), Ok(Received { sequence, message }));
}

#[test]
fn hello() {
    let hello = Message::Hello(Hello {
        protocol_version: 7,
        options: Options {
            authentication: true,
            unknown: 0,
        },
    });
    check(hello, HELLO, &[0x07, 0x01, 0x00, 0x00, 0x00]);
}

#[test]
fn hello_keeps_unknown_option_bits() {
    // The drone answers unsupported options with HelloReply "option not supported", so it must see them.
    let payload = [1, 0x21, 0x00, 0x00, 0x80];
    let decoded = decode(HELLO, &payload).unwrap();
    let options = Options {
        authentication: true,
        unknown: 0x8000_0020,
    };
    assert_eq!(
        decoded,
        Message::Hello(Hello {
            protocol_version: 1,
            options
        })
    );
    assert_eq!(encode(&decoded, 0), frame(0, HELLO, 0, &payload));
}

#[test]
fn hello_reply() {
    let reply = Message::HelloReply(HelloReply {
        protocol_version: 1,
        result: HelloResult::OptionNotSupported,
        options: Options::default(),
    });
    check(reply, HELLO_REPLY, &[0x01, 0x02, 0x00, 0x00, 0x00, 0x00]);
}

#[test]
fn hello_with_other_version_still_decodes() {
    // The drone answers with HelloReply "version mismatch", so it must see the version.
    let decoded = decode(HELLO, &[2, 0, 0, 0, 0]);
    assert_eq!(
        decoded,
        Ok(Message::Hello(Hello {
            protocol_version: 2,
            options: Options::default()
        }))
    );
}

#[test]
fn hello_and_hello_reply_always_use_sequence_0() {
    // protocol.md §5.1: they carry Sequence 0, whatever the sender's counter says.
    let hello = Message::Hello(Hello {
        protocol_version: 1,
        options: Options::default(),
    });
    assert_eq!(read_one(&encode(&hello, 9)).map(|r| return r.sequence), Ok(0));
    for (message_type, payload) in [(HELLO, vec![1, 0, 0, 0, 0]), (HELLO_REPLY, vec![1, 0, 0, 0, 0, 0])] {
        let decoded = read_one(&frame(0, message_type, 7, &payload));
        assert_eq!(
            decoded,
            Err(FrameError::InvalidValue),
            "type {message_type} with sequence 7"
        );
    }
}

#[test]
fn hello_reply_every_result() {
    for (result, byte) in [
        (HelloResult::Ok, 0),
        (HelloResult::VersionMismatch, 1),
        (HelloResult::OptionNotSupported, 2),
    ] {
        let reply = Message::HelloReply(HelloReply {
            protocol_version: 1,
            result,
            options: Options::default(),
        });
        check(reply, HELLO_REPLY, &[1, byte, 0, 0, 0, 0]);
    }
}

#[test]
fn hello_reply_rejects_unknown_result() {
    for byte in [3, 0xFF] {
        let decoded = decode(HELLO_REPLY, &[1, byte, 0, 0, 0, 0]);
        assert_eq!(decoded, Err(FrameError::InvalidValue), "result {byte}");
    }
}

#[test]
fn ping() {
    check(
        Message::Ping(Ping { timestamp: 0x11223344 }),
        PING,
        &[0x44, 0x33, 0x22, 0x11],
    );
}

#[test]
fn pong() {
    let pong = Message::Pong(Pong {
        ping_time: 1,
        receive_time: 0x0100,
        send_time: 0xFFFF_FFFF,
    });
    check(pong, PONG, &[1, 0, 0, 0, 0x00, 0x01, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF]);
}

#[test]
fn rejects_wrong_payload_length() {
    let cases: [(u8, usize); 4] = [(HELLO, 5), (HELLO_REPLY, 6), (PING, 4), (PONG, 12)];
    for (message_type, length) in cases {
        for wrong in [0, length - 1, length + 1, MAX_PAYLOAD_LEN] {
            let decoded = decode(message_type, &vec![0; wrong]);
            assert_eq!(
                decoded,
                Err(FrameError::WrongLength),
                "type {message_type} with {wrong} bytes"
            );
        }
    }
}
