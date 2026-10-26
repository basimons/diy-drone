//! Test helpers that build packets and frames by hand, independently of the library's own CRC and COBS code.
#![allow(dead_code)] // each test file uses a different subset

use drone_common::wire::message::Message;
use drone_common::wire::message::MessageError;
use drone_common::wire::packet::{MAX_PACKET_LEN, Packet, PacketError};
use drone_common::wire::stream::{self, MAX_FRAME_LEN, StreamDecoder};

/// CRC-16/CCITT-FALSE, written out bit by bit.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        for bit in (0..8).rev() {
            let feedback = ((crc >> 15) as u8 ^ (byte >> bit)) & 1 == 1;
            crc <<= 1;
            if feedback {
                crc ^= 0x1021;
            }
        }
    }
    return crc;
}

/// Plain COBS, followed by the `0x00` delimiter.
pub fn cobs_frame(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0];
    let mut code_at = 0;
    for (i, &byte) in data.iter().enumerate() {
        if byte != 0 {
            out.push(byte);
        }
        // A full block (254 data bytes) only opens a new one if more data follows.
        if byte == 0 || (out.len() - code_at == 255 && i + 1 < data.len()) {
            out[code_at] = (out.len() - code_at) as u8;
            code_at = out.len();
            out.push(0);
        }
    }
    out[code_at] = (out.len() - code_at) as u8;
    out.push(0);
    return out;
}

/// Packet bytes (flags, type, sequence, payload) plus a correct CRC.
pub fn packet(flags: u8, message_type: u8, sequence: u16, payload: &[u8]) -> Vec<u8> {
    let mut bytes = vec![flags, message_type];
    bytes.extend_from_slice(&sequence.to_le_bytes());
    bytes.extend_from_slice(payload);
    return with_crc(bytes);
}

pub fn with_crc(bytes: Vec<u8>) -> Vec<u8> {
    let crc = crc16(&bytes);
    return [bytes, crc.to_le_bytes().to_vec()].concat();
}

/// `packet(..)`, COBS-framed for a byte stream.
pub fn frame(flags: u8, message_type: u8, sequence: u16, payload: &[u8]) -> Vec<u8> {
    return cobs_frame(&packet(flags, message_type, sequence, payload));
}

pub fn encode_packet(message: &Message, sequence: u16) -> Vec<u8> {
    let mut out = [0u8; MAX_PACKET_LEN];
    return message.encode(sequence, &mut out).as_bytes().to_vec();
}

pub fn encode_frame(message: &Message, sequence: u16) -> Vec<u8> {
    let mut packet = [0u8; MAX_PACKET_LEN];
    let mut frame = [0u8; MAX_FRAME_LEN];
    return stream::encode(&message.encode(sequence, &mut packet), &mut frame).to_vec();
}

/// Feeds `bytes` to one decoder in chunks of `chunk_size`; returns the bytes of every packet, in order.
pub fn split(bytes: &[u8], chunk_size: usize) -> Vec<Result<Vec<u8>, PacketError>> {
    let mut decoder = StreamDecoder::new();
    let mut packets = Vec::new();
    for mut chunk in bytes.chunks(chunk_size) {
        while let Some(packet) = decoder.next_packet(&mut chunk) {
            packets.push(packet.map(|p| return p.as_bytes().to_vec()));
        }
        assert!(chunk.is_empty(), "next_packet returned None before using up the chunk");
    }
    return packets;
}

/// Splits `bytes` as one chunk and returns every packet's sequence number and decoded message.
pub fn read_all(bytes: &[u8]) -> Vec<Result<(u16, Message), PacketError>> {
    let mut decoder = StreamDecoder::new();
    let mut chunk = bytes;
    let mut results = Vec::new();
    while let Some(packet) = decoder.next_packet(&mut chunk) {
        results.push(packet.map(|p| return (p.sequence(), Message::decode(&p).expect("valid message"))));
    }
    return results;
}

/// Decodes a hand-built packet with sequence 0.
pub fn decode_payload(message_type: u8, payload: &[u8]) -> Result<Message, MessageError> {
    let bytes = packet(0, message_type, 0, payload);
    return Message::decode(&Packet::parse(&bytes).unwrap());
}

/// Encodes `message` with sequence 42, checks it against a hand-built packet, and checks it decodes back.
pub fn check_round_trip(message: Message, message_type: u8, payload: &[u8]) {
    let encoded = encode_packet(&message, 42);
    assert_eq!(encoded, packet(0, message_type, 42, payload), "{message:?}");
    assert_eq!(decode_payload(message_type, payload), Ok(message));
}

/// `valid` decodes, but the same payload cut short or with bytes added is `WrongLength`.
pub fn check_only_length(message_type: u8, valid: &[u8]) {
    assert!(
        decode_payload(message_type, valid).is_ok(),
        "type {message_type:#04x}: {valid:?} should decode"
    );
    let longest = [valid, &[0; 249][valid.len()..]].concat();
    for wrong in [0, valid.len().saturating_sub(1), valid.len() + 1, 249] {
        if wrong == valid.len() {
            continue;
        }
        let payload = if wrong < valid.len() {
            &valid[..wrong]
        } else {
            &longest[..wrong]
        };
        assert_eq!(
            decode_payload(message_type, payload),
            Err(MessageError::WrongLength),
            "type {message_type:#04x} with {wrong} bytes"
        );
    }
}
