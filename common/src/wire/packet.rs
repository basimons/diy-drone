//! A packet: one message on the wire, before any stream framing (protocol.md §4.1).
//! `flags | type | sequence | payload | CRC-16`, at most 255 bytes. Over UDP one datagram is one packet.

use crate::wire::codes::MessageType;
use crate::wire::crc16::crc16;

const HEADER_LEN: usize = 4; // flags, type, sequence
const CRC_LEN: usize = 2;

pub const MAX_PACKET_LEN: usize = 255;
pub(crate) const MAX_PAYLOAD_LEN: usize = MAX_PACKET_LEN - HEADER_LEN - CRC_LEN;

/// Why bytes are not an intact packet. Dropped silently and counted for link-quality stats (protocol.md §4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PacketError {
    TooShort,
    TooLong,
    BadCobs, // only from a byte stream: the COBS stuffing is broken
    BadCrc,
    UnsupportedFlags, // only allowed once authentication exists
}

impl core::fmt::Display for PacketError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let text = match self {
            PacketError::TooShort => "packet shorter than 6 bytes",
            PacketError::TooLong => "packet longer than 255 bytes",
            PacketError::BadCobs => "broken COBS framing",
            PacketError::BadCrc => "CRC-16 mismatch",
            PacketError::UnsupportedFlags => "flags set, but authentication is not supported yet",
        };
        return f.write_str(text);
    }
}

impl core::error::Error for PacketError {}

/// Bytes that passed the length, CRC and flags checks. Made only by `Packet::parse`, `Message::encode` and
/// `StreamDecoder`, so `Message::decode` never sees unchecked bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Packet<'a> {
    bytes: &'a [u8],
}

impl<'a> Packet<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self, PacketError> {
        if bytes.len() < HEADER_LEN + CRC_LEN {
            return Err(PacketError::TooShort);
        }
        if bytes.len() > MAX_PACKET_LEN {
            return Err(PacketError::TooLong);
        }
        let (body, crc) = bytes.split_at(bytes.len() - CRC_LEN);
        if crc16(body) != u16::from_le_bytes([crc[0], crc[1]]) {
            return Err(PacketError::BadCrc);
        }
        if body[0] != 0 {
            return Err(PacketError::UnsupportedFlags);
        }
        let packet = Self { bytes };
        debug_assert!(packet.payload().len() <= MAX_PAYLOAD_LEN);
        return Ok(packet);
    }

    /// Writes a packet into `out`. Our own message encoder is the only caller.
    pub(crate) fn write(
        message_type: MessageType,
        sequence: u16,
        payload: &[u8],
        out: &'a mut [u8; MAX_PACKET_LEN],
    ) -> Self {
        // `Unknown` only comes out of decoding; sending it would decode as a different type.
        debug_assert!(!matches!(message_type, MessageType::Unknown(_)), "{message_type:?}");
        debug_assert!(payload.len() <= MAX_PAYLOAD_LEN, "payload of {} bytes", payload.len());
        let crc_at = HEADER_LEN + payload.len();
        out[0] = 0; // flags
        out[1] = u8::from(message_type);
        out[2..HEADER_LEN].copy_from_slice(&sequence.to_le_bytes());
        out[HEADER_LEN..crc_at].copy_from_slice(payload);
        let crc = crc16(&out[..crc_at]);
        out[crc_at..crc_at + CRC_LEN].copy_from_slice(&crc.to_le_bytes());
        let bytes = &out[..crc_at + CRC_LEN];
        debug_assert_eq!(Packet::parse(bytes), Ok(Self { bytes }), "written packet doesn't parse");
        return Self { bytes };
    }

    pub fn sequence(&self) -> u16 {
        return u16::from_le_bytes([self.bytes[2], self.bytes[3]]);
    }

    /// The bytes to send, e.g. as one UDP datagram.
    pub fn as_bytes(&self) -> &'a [u8] {
        return self.bytes;
    }

    pub(crate) fn message_type(&self) -> MessageType {
        return MessageType::from(self.bytes[1]);
    }

    pub(crate) fn payload(&self) -> &'a [u8] {
        return &self.bytes[HEADER_LEN..self.bytes.len() - CRC_LEN];
    }
}
