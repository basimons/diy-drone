//! Reading and writing the little-endian fields of a payload.

use crate::wire::message::MessageError;
use crate::wire::packet::MAX_PAYLOAD_LEN;

pub(crate) fn invalid<E>(_: E) -> MessageError {
    return MessageError::InvalidValue;
}

/// Reads fields from the front of a payload.
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        return Self { bytes };
    }

    pub(crate) fn is_empty(&self) -> bool {
        return self.bytes.is_empty();
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], MessageError> {
        let (head, rest) = self.bytes.split_first_chunk::<N>().ok_or(MessageError::WrongLength)?;
        self.bytes = rest;
        return Ok(*head);
    }

    pub(crate) fn u8(&mut self) -> Result<u8, MessageError> {
        return Ok(self.take::<1>()?[0]);
    }

    pub(crate) fn u16(&mut self) -> Result<u16, MessageError> {
        return Ok(u16::from_le_bytes(self.take()?));
    }

    pub(crate) fn i16(&mut self) -> Result<i16, MessageError> {
        return Ok(i16::from_le_bytes(self.take()?));
    }

    pub(crate) fn u32(&mut self) -> Result<u32, MessageError> {
        return Ok(u32::from_le_bytes(self.take()?));
    }

    pub(crate) fn rest(&mut self) -> &'a [u8] {
        return core::mem::take(&mut self.bytes);
    }
}

/// Writes fields into a payload buffer.
pub(crate) struct Writer {
    bytes: [u8; MAX_PAYLOAD_LEN],
    len: usize,
}

impl Writer {
    pub(crate) fn new() -> Self {
        return Self {
            bytes: [0; MAX_PAYLOAD_LEN],
            len: 0,
        };
    }

    pub(crate) fn put(&mut self, bytes: &[u8]) {
        debug_assert!(
            self.len + bytes.len() <= MAX_PAYLOAD_LEN,
            "{} + {} bytes",
            self.len,
            bytes.len()
        );
        self.bytes[self.len..self.len + bytes.len()].copy_from_slice(bytes);
        self.len += bytes.len();
    }

    pub(crate) fn u8(&mut self, value: u8) {
        self.put(&[value]);
    }

    pub(crate) fn u16(&mut self, value: u16) {
        self.put(&value.to_le_bytes());
    }

    pub(crate) fn i16(&mut self, value: i16) {
        self.put(&value.to_le_bytes());
    }

    pub(crate) fn u32(&mut self, value: u32) {
        self.put(&value.to_le_bytes());
    }

    pub(crate) fn written(&self) -> &[u8] {
        return &self.bytes[..self.len];
    }
}
