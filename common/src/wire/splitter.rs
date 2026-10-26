//! Cutting a received byte stream (serial, radio) into frames at the `0x00` delimiters (protocol.md §4.2).
//! Transports that deliver whole frames (one UDP packet = one frame) don't need this.

use crate::wire::message::{DecodeError, MAX_ENCODED_LEN};

#[derive(Debug)]
pub struct FrameSplitter {
    buffer: [u8; MAX_ENCODED_LEN],
    len: usize,
    returned: bool, // the buffer holds a frame handed out by the last call
    overflowed: bool,
}

impl FrameSplitter {
    pub fn new() -> Self {
        return Self {
            buffer: [0; MAX_ENCODED_LEN],
            len: 0,
            returned: false,
            overflowed: false,
        };
    }

    /// Takes bytes off the front of `chunk` until a frame is complete and returns it, `0x00` included, ready for
    /// `Message::decode`. Returns `None` once `chunk` is used up; a partial frame is kept for the next chunk.
    /// Empty frames (repeated delimiters) are skipped. A frame longer than `MAX_ENCODED_LEN` is dropped and reported
    /// once, as `TooLong`, when its delimiter arrives.
    pub fn next_frame(&mut self, chunk: &mut &[u8]) -> Option<Result<&[u8], DecodeError>> {
        if self.returned {
            self.len = 0;
            self.returned = false;
        }
        loop {
            let Some(end) = chunk.iter().position(|&b| return b == 0) else {
                self.store(chunk);
                *chunk = &[];
                return None;
            };
            self.store(&chunk[..end]);
            *chunk = &chunk[end + 1..];
            if self.overflowed {
                self.len = 0;
                self.overflowed = false;
                return Some(Err(DecodeError::TooLong));
            }
            if self.len > 0 {
                self.buffer[self.len] = 0;
                self.len += 1;
                self.returned = true;
                return Some(Ok(&self.buffer[..self.len]));
            }
        }
    }

    fn store(&mut self, bytes: &[u8]) {
        let room = MAX_ENCODED_LEN - 1 - self.len; // keep one byte for the delimiter
        if bytes.len() > room {
            self.overflowed = true;
        }
        if !self.overflowed {
            self.buffer[self.len..self.len + bytes.len()].copy_from_slice(bytes);
            self.len += bytes.len();
        }
    }
}

impl Default for FrameSplitter {
    fn default() -> Self {
        return Self::new();
    }
}
