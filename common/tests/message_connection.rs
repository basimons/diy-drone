use drone_common::wire::codes::{HelloResult, MessageType};
use drone_common::wire::crc16::crc16;
use drone_common::wire::envelope::Envelope;
use drone_common::wire::message::*;

fn envelope(message_type: MessageType, payload: &[u8]) -> Envelope {
    return Envelope::new(message_type, 0, payload).unwrap();
}

/// Encodes `message`, checks the envelope's type and payload bytes, and checks it decodes back to `message`.
fn check(message: Message, message_type: MessageType, payload: &[u8]) {
    let encoded = message.encode(42);
    assert_eq!(encoded.message_type(), message_type);
    assert_eq!(encoded.sequence(), 42);
    assert_eq!(encoded.payload(), payload);
    assert_eq!(Message::decode(&encoded), Ok(message));
}

#[test]
fn hello() {
    let hello = Message::Hello(Hello {
        protocol_version: 0x0102,
        options: 0x0A0B0C0D,
    });
    check(hello, MessageType::Hello, &[0x02, 0x01, 0x0D, 0x0C, 0x0B, 0x0A]);
}

#[test]
fn hello_with_options_still_decodes() {
    // The drone answers unsupported options with HelloReply "option not supported", so it must see them.
    let decoded = Message::decode(&envelope(MessageType::Hello, &[1, 0, 0xFF, 0xFF, 0xFF, 0xFF]));
    assert_eq!(
        decoded,
        Ok(Message::Hello(Hello {
            protocol_version: 1,
            options: 0xFFFF_FFFF
        }))
    );
}

#[test]
fn hello_reply() {
    let reply = Message::HelloReply(HelloReply {
        protocol_version: 1,
        result: HelloResult::OptionNotSupported,
        options: 0x0000_0002,
    });
    check(
        reply,
        MessageType::HelloReply,
        &[0x01, 0x00, 0x02, 0x02, 0x00, 0x00, 0x00],
    );
}

#[test]
fn hello_reply_rejects_unknown_result() {
    let decoded = Message::decode(&envelope(MessageType::HelloReply, &[1, 0, 3, 0, 0, 0, 0]));
    assert_eq!(decoded, Err(DecodeError::InvalidValue));
}

#[test]
fn ping() {
    check(
        Message::Ping(Ping { timestamp: 0x11223344 }),
        MessageType::Ping,
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
    check(
        pong,
        MessageType::Pong,
        &[1, 0, 0, 0, 0x00, 0x01, 0, 0, 0xFF, 0xFF, 0xFF, 0xFF],
    );
}

#[test]
fn rejects_wrong_payload_length() {
    let cases: [(MessageType, usize); 4] = [
        (MessageType::Hello, 6),
        (MessageType::HelloReply, 7),
        (MessageType::Ping, 4),
        (MessageType::Pong, 12),
    ];
    for (message_type, length) in cases {
        for wrong in [0, length - 1, length + 1] {
            let decoded = Message::decode(&envelope(message_type, &vec![0; wrong]));
            assert_eq!(
                decoded,
                Err(DecodeError::WrongLength),
                "{message_type:?} with {wrong} bytes"
            );
        }
    }
}

#[test]
fn unknown_type_is_reported_with_its_byte() {
    // Only decoding produces an Unknown type, so build the envelope from bytes.
    let mut bytes = vec![0x00, 0xEE, 0x00, 0x00];
    bytes.extend_from_slice(&crc16(&bytes).to_le_bytes());
    let unknown = Envelope::decode(&bytes).unwrap();
    assert_eq!(Message::decode(&unknown), Err(DecodeError::UnknownType(0xEE)));
}
