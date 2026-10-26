mod support;

use drone_common::wire::codes::HelloResult;
use drone_common::wire::message::*;
use drone_common::wire::packet::Packet;
use support::*;

const HELLO: u8 = 0x01;
const HELLO_REPLY: u8 = 0x02;
const PING: u8 = 0x03;
const PONG: u8 = 0x04;
const MAX_PAYLOAD_LEN: usize = 249; // protocol.md §4.1

fn decode_with_sequence(message_type: u8, sequence: u16, payload: &[u8]) -> Result<Message, MessageError> {
    let bytes = packet(0, message_type, sequence, payload);
    return Message::decode(&Packet::parse(&bytes).unwrap());
}

/// Decodes a hand-built packet with sequence 0.
fn decode(message_type: u8, payload: &[u8]) -> Result<Message, MessageError> {
    return decode_with_sequence(message_type, 0, payload);
}

/// Encodes `message`, checks it against a hand-built packet, and checks it decodes back to `message`.
fn check(message: Message, message_type: u8, payload: &[u8]) {
    let handshake = message_type == HELLO || message_type == HELLO_REPLY;
    let sequence = if handshake { 0 } else { 42 };
    let encoded = encode_packet(&message, 42);
    assert_eq!(encoded, packet(0, message_type, sequence, payload));
    assert_eq!(Message::decode(&Packet::parse(&encoded).unwrap()), Ok(message));
}

#[test]
fn hello() {
    let hello = Message::Hello(Hello {
        protocol_version: 7,
        options: Options {
            authentication: true,
            unknown: UnknownOptions::default(),
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
        unknown: UnknownOptions::new(0x8000_0020).unwrap(),
    };
    assert_eq!(options.unknown.bits(), 0x8000_0020);
    assert_eq!(
        decoded,
        Message::Hello(Hello {
            protocol_version: 1,
            options
        })
    );
    assert_eq!(encode_packet(&decoded, 0), packet(0, HELLO, 0, &payload));
}

#[test]
fn unknown_options_never_hold_a_known_bit() {
    // Otherwise encoding would drop the bit, and the message wouldn't decode back the same.
    assert_eq!(UnknownOptions::new(0x0000_0001), None); // authentication
    assert_eq!(UnknownOptions::new(0xFFFF_FFFF), None);
    assert_eq!(UnknownOptions::new(0).map(|o| return o.bits()), Some(0));
    assert_eq!(
        UnknownOptions::new(0xFFFF_FFFE).map(|o| return o.bits()),
        Some(0xFFFF_FFFE)
    );
    assert_eq!(UnknownOptions::default().bits(), 0);
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
    assert_eq!(Packet::parse(&encode_packet(&hello, 9)).unwrap().sequence(), 0);
    for (message_type, payload) in [(HELLO, vec![1, 0, 0, 0, 0]), (HELLO_REPLY, vec![1, 0, 0, 0, 0, 0])] {
        let decoded = decode_with_sequence(message_type, 7, &payload);
        assert_eq!(
            decoded,
            Err(MessageError::InvalidValue),
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
        assert_eq!(decoded, Err(MessageError::InvalidValue), "result {byte}");
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
                Err(MessageError::WrongLength),
                "type {message_type} with {wrong} bytes"
            );
        }
    }
}

#[test]
fn unknown_type_is_reported_with_its_byte() {
    // The drone answers with Reply "unknown type", so it must see the byte.
    assert_eq!(decode(0xEE, &[]), Err(MessageError::UnknownType(0xEE)));
}
