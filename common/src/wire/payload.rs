//! Reading and writing the little-endian fields of a payload. `Wire` and `Code` are public but live in this private
//! module, so other crates use the types that implement them without being able to call or implement them.

use crate::wire::message::MessageError;
use crate::wire::packet::MAX_PAYLOAD_LEN;

/// A value with a fixed place in a payload: reads and writes itself.
pub trait Wire: Sized {
    fn read(r: &mut Reader) -> Result<Self, MessageError>;

    fn write(&self, w: &mut Writer);
}

/// An enum stored on the wire as a number. A number that isn't one of its values doesn't decode: the protocol
/// version changes when codes are added, so a peer that passed Hello never sends one (protocol.md §7.1).
pub trait Code: Copy {
    type Repr: Wire + Copy;

    fn from_repr(value: Self::Repr) -> Result<Self, MessageError>;

    fn repr(self) -> Self::Repr;
}

impl<C: Code> Wire for C {
    fn read(r: &mut Reader) -> Result<Self, MessageError> {
        return C::from_repr(C::Repr::read(r)?);
    }

    fn write(&self, w: &mut Writer) {
        self.repr().write(w);
    }
}

/// Implements `Wire` for a struct whose fields are written one after the other, in the order listed. The fields are
/// listed once for both directions, and the struct literal makes the compiler check none is missing.
macro_rules! impl_wire {
    ($name:ident { $($field:ident),* }) => {
        impl $crate::wire::payload::Wire for $name {
            fn read(_r: &mut $crate::wire::payload::Reader) -> Result<Self, $crate::wire::message::MessageError> {
                return Ok(Self { $($field: $crate::wire::payload::Wire::read(_r)?,)* });
            }

            fn write(&self, _w: &mut $crate::wire::payload::Writer) {
                $($crate::wire::payload::Wire::write(&self.$field, _w);)*
            }
        }
    };
}
pub(crate) use impl_wire;

macro_rules! wire_number {
    ($($ty:ty),*) => {$(
        impl Wire for $ty {
            fn read(r: &mut Reader) -> Result<Self, MessageError> {
                return Ok(<$ty>::from_le_bytes(r.take()?));
            }

            fn write(&self, w: &mut Writer) {
                w.put(&self.to_le_bytes());
            }
        }
    )*};
}
wire_number!(u8, u16, i16, u32);

pub(crate) fn invalid<E>(_: E) -> MessageError {
    return MessageError::InvalidValue;
}

/// Reads fields from the front of a payload.
pub struct Reader<'a> {
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

    pub(crate) fn rest(&mut self) -> &'a [u8] {
        return core::mem::take(&mut self.bytes);
    }
}

/// Writes fields into a payload buffer.
pub struct Writer {
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

    pub(crate) fn written(&self) -> &[u8] {
        return &self.bytes[..self.len];
    }
}
