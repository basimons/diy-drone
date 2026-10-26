//! Errors print readably and work as `core::error::Error`, e.g. with `?` into `anyhow` in the client.
use drone_common::wire::message::{MessageError, UnknownType};
use drone_common::wire::packet::PacketError;

fn is_error<E: core::error::Error>(_: &E) -> bool {
    return true;
}

#[test]
fn packet_errors_print_readably() {
    let cases = [
        (PacketError::TooShort, "packet shorter than 6 bytes"),
        (PacketError::TooLong, "packet longer than 255 bytes"),
        (PacketError::BadCobs, "broken COBS framing"),
        (PacketError::BadCrc, "CRC-16 mismatch"),
        (
            PacketError::UnsupportedFlags,
            "flags set, but authentication is not supported yet",
        ),
    ];
    for (error, text) in cases {
        assert_eq!(error.to_string(), text);
        assert!(is_error(&error));
    }
}

#[test]
fn message_errors_print_readably() {
    let cases = [
        (
            MessageError::UnknownType(UnknownType::new(0xEE).unwrap()),
            "unknown message type 0xee",
        ),
        (
            MessageError::UnknownCode {
                kind: "ArmRefusal",
                value: 9,
            },
            "unknown ArmRefusal 9",
        ),
        (
            MessageError::WrongLength,
            "payload length doesn't match the message type",
        ),
        (
            MessageError::InvalidValue,
            "a field holds a value the protocol doesn't allow",
        ),
    ];
    for (error, text) in cases {
        assert_eq!(error.to_string(), text);
        assert!(is_error(&error));
    }
}
