mod support;

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
    let encoded = encode_packet(&message, sequence);
    assert_eq!(encoded, packet(0, message_type, sequence, payload));
    assert_eq!(Message::decode(&Packet::parse(&encoded).unwrap()), Ok(message));
}

#[test]
fn hello() {
    let hello = Message::Hello(Hello {
        protocol_version: 7,
        options: HelloOptions {
            known: Options { authentication: true },
            unknown: UnknownOptions::default(),
        },
    });
    check(hello, HELLO, &[0x07, 0x01, 0x00, 0x00, 0x00]);
}

/// The options of a Hello decoded from `payload`.
fn hello_options(payload: &[u8]) -> HelloOptions {
    let Ok(Message::Hello(hello)) = decode(HELLO, payload) else {
        panic!("{payload:?} is not a Hello");
    };
    return hello.options;
}

#[test]
fn hello_keeps_unknown_option_bits() {
    // The drone answers unsupported options with HelloReply "option not supported", so it must see them.
    let payload = [1, 0x21, 0x00, 0x00, 0x80];
    let options = hello_options(&payload);
    assert_eq!(options.known, Options { authentication: true });
    assert_eq!(options.unknown.bits(), 0x8000_0020);
    assert_eq!(
        encode_packet(&decode(HELLO, &payload).unwrap(), 0),
        packet(0, HELLO, 0, &payload)
    );
}

#[test]
fn unknown_options_never_hold_a_known_bit() {
    // Otherwise encoding would drop the bit, and the message wouldn't decode back the same.
    let options = hello_options(&[1, 0xFF, 0xFF, 0xFF, 0xFF]);
    assert_eq!(options.known, Options { authentication: true });
    assert_eq!(options.unknown.bits(), 0xFFFF_FFFE);
    assert_eq!(UnknownOptions::default().bits(), 0);
}

#[test]
fn hello_reply_only_holds_options_the_drone_knows() {
    // protocol.md §5.1: the options the drone accepted, and it can't accept a bit without a meaning.
    assert_eq!(
        decode(HELLO_REPLY, &[1, 0, 0x20, 0, 0, 0]),
        Err(MessageError::InvalidValue)
    );
    assert_eq!(
        decode(HELLO_REPLY, &[1, 0, 0, 0, 0, 0x80]),
        Err(MessageError::InvalidValue)
    );
    let accepted = decode(HELLO_REPLY, &[1, 0, 0x01, 0, 0, 0]);
    let reply = HelloReply {
        protocol_version: 1,
        result: HelloResult::Ok,
        options: Options { authentication: true },
    };
    assert_eq!(accepted, Ok(Message::HelloReply(reply)));
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
            options: HelloOptions::default()
        }))
    );
}

#[test]
fn hello_and_hello_reply_only_decode_with_sequence_0() {
    // protocol.md §5.1: they are message 0 of a session.
    for (message_type, payload) in [(HELLO, vec![1, 0, 0, 0, 0]), (HELLO_REPLY, vec![1, 0, 0, 0, 0, 0])] {
        assert!(decode_with_sequence(message_type, 0, &payload).is_ok());
        let decoded = decode_with_sequence(message_type, 7, &payload);
        assert_eq!(
            decoded,
            Err(MessageError::InvalidValue),
            "type {message_type} with sequence 7"
        );
    }
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "message 0 of a session")]
fn encoding_hello_with_another_sequence_is_a_bug() {
    // The caller resets its counter at the handshake (protocol.md §6), so anything else is a bug in the caller.
    let hello = Message::Hello(Hello {
        protocol_version: 1,
        options: HelloOptions::default(),
    });
    encode_packet(&hello, 9);
}

#[test]
#[cfg(not(debug_assertions))]
fn a_handshake_is_always_encoded_as_message_0() {
    // In release the caller's wrong sequence isn't reported, but the packet is still one the other side accepts.
    let hello = Message::Hello(Hello {
        protocol_version: 1,
        options: HelloOptions::default(),
    });
    assert_eq!(encode_packet(&hello, 9), packet(0, HELLO, 0, &[1, 0, 0, 0, 0]));
    let reply = Message::HelloReply(HelloReply {
        protocol_version: 1,
        result: HelloResult::Ok,
        options: Options::default(),
    });
    assert_eq!(encode_packet(&reply, 9), packet(0, HELLO_REPLY, 0, &[1, 0, 0, 0, 0, 0]));
}

#[test]
#[cfg(debug_assertions)]
#[should_panic(expected = "message 0 of a session")]
fn encoding_hello_reply_with_another_sequence_is_a_bug() {
    let reply = Message::HelloReply(HelloReply {
        protocol_version: 1,
        result: HelloResult::Ok,
        options: Options::default(),
    });
    encode_packet(&reply, 9);
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
    // HelloReply's results never change between protocol versions (protocol.md §5.1).
    let decoded = decode(HELLO_REPLY, &[1, 0xFF, 0, 0, 0, 0]);
    let unknown = MessageError::UnknownCode {
        kind: "HelloResult",
        value: 0xFF,
    };
    assert_eq!(decoded, Err(unknown));
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
    let unknown = UnknownType::new(0xEE).unwrap();
    assert_eq!(decode(0xEE, &[]), Err(MessageError::UnknownType(unknown)));
}

#[test]
fn type_without_a_rust_message_yet_is_unknown_to_this_build() {
    let link_test = UnknownType::new(0x05).unwrap();
    assert_eq!(decode(0x05, &[]), Err(MessageError::UnknownType(link_test)));
}
