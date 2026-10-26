//! Packets over a byte stream (serial, radio), protocol.md §4.2: each packet is COBS-encoded so it contains no
//! `0x00`, then followed by one `0x00` that ends the frame. Transports that deliver whole packets (UDP) skip this.

use crate::wire::cobs::{self, CobsError};
use crate::wire::packet::{MAX_PACKET_LEN, Packet, PacketError};

/// COBS-encoded maximum packet plus the `0x00` delimiter.
pub const MAX_FRAME_LEN: usize = cobs::max_encoded_len(MAX_PACKET_LEN) + 1;

/// The frame to write to the stream: `packet` COBS-encoded, `0x00` delimiter included.
pub fn encode<'a>(packet: &Packet, out: &'a mut [u8; MAX_FRAME_LEN]) -> &'a [u8] {
    let len = cobs::encode(packet.as_bytes(), &mut out[..MAX_FRAME_LEN - 1]).expect("MAX_FRAME_LEN fits any packet");
    out[len] = 0;
    return &out[..len + 1];
}

/// Cuts a received byte stream into frames at the `0x00` delimiters and turns each back into a packet.
#[derive(Debug)]
pub struct StreamDecoder {
    frame: [u8; MAX_FRAME_LEN - 1], // the frame being received, without its delimiter
    len: usize,
    overflowed: bool,
    packet: [u8; MAX_PACKET_LEN], // the last decoded packet, borrowed by what `next_packet` returns
}

impl StreamDecoder {
    pub const fn new() -> Self {
        return Self {
            frame: [0; MAX_FRAME_LEN - 1],
            len: 0,
            overflowed: false,
            packet: [0; MAX_PACKET_LEN],
        };
    }

    /// Takes bytes off the front of `chunk` until a frame is complete and returns its packet. Returns `None` once
    /// `chunk` is used up; a partial frame is kept for the next chunk. Empty frames (repeated delimiters) are
    /// skipped. A frame longer than `MAX_FRAME_LEN` is reported once, as `TooLong`, when its delimiter arrives.
    pub fn next_packet(&mut self, chunk: &mut &[u8]) -> Option<Result<Packet<'_>, PacketError>> {
        loop {
            let Some(end) = chunk.iter().position(|&b| return b == 0) else {
                self.store(chunk);
                *chunk = &[];
                return None;
            };
            self.store(&chunk[..end]);
            *chunk = &chunk[end + 1..];
            let len = core::mem::take(&mut self.len);
            if core::mem::take(&mut self.overflowed) {
                return Some(Err(PacketError::TooLong));
            }
            if len > 0 {
                return Some(self.decode(len));
            }
        }
    }

    fn store(&mut self, bytes: &[u8]) {
        if bytes.len() > self.frame.len() - self.len {
            self.overflowed = true;
        }
        if !self.overflowed {
            self.frame[self.len..self.len + bytes.len()].copy_from_slice(bytes);
            self.len += bytes.len();
        }
    }

    fn decode(&mut self, len: usize) -> Result<Packet<'_>, PacketError> {
        let packet_len = cobs::decode(&self.frame[..len], &mut self.packet).map_err(|e| {
            return match e {
                CobsError::OutputTooSmall => PacketError::TooLong,
                CobsError::ZeroByte | CobsError::Truncated => PacketError::BadCobs,
            };
        })?;
        return Packet::parse(&self.packet[..packet_len]);
    }
}

impl Default for StreamDecoder {
    fn default() -> Self {
        return Self::new();
    }
}
