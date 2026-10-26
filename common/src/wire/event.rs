//! Event (protocol.md §5.4): the drone tells the client something happened. One type per event, holding only
//! the data that event has; the level and code of a known event are fixed.

use core::num::NonZeroU16;

use crate::wire::codes::{EventCode, EventLevel, FailsafeCause, Unknown};
use crate::wire::message::MessageError;
use crate::wire::packet::MAX_PAYLOAD_LEN;
use crate::wire::payload::{Reader, Writer, invalid};

/// The rest of the payload after Level and Code.
pub const MAX_EVENT_DATA_LEN: usize = MAX_PAYLOAD_LEN - 3;

/// What every known event type has: its code and level on the wire.
pub trait EventKind {
    const CODE: EventCode;
    const LEVEL: EventLevel;
}

/// Free text from the drone, sent with Code 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Text {
    pub level: EventLevel,
    pub text: EventText,
}

/// An event this build doesn't know (from a newer drone), kept so the caller can e.g. log it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UnknownEvent {
    pub level: EventLevel,
    pub code: Unknown<EventCode>,
    pub data: EventData,
}

/// Up to `MAX_EVENT_DATA_LEN` bytes, owned (protocol.md §7.1).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Bytes {
    bytes: [u8; MAX_EVENT_DATA_LEN], // zero after `len`, so the derived `PartialEq` compares the content only
    len: usize,
}

impl Bytes {
    fn new(bytes: &[u8]) -> Option<Self> {
        if bytes.len() > MAX_EVENT_DATA_LEN {
            return None;
        }
        return Some(Self {
            bytes: core::array::from_fn(|i| return bytes.get(i).copied().unwrap_or(0)),
            len: bytes.len(),
        });
    }

    fn as_slice(&self) -> &[u8] {
        debug_assert!(self.len <= MAX_EVENT_DATA_LEN, "length {}", self.len);
        debug_assert!(
            self.bytes[self.len..].iter().all(|&b| return b == 0),
            "PartialEq needs zero padding"
        );
        return &self.bytes[..self.len];
    }
}

/// UTF-8 text of 1 to `MAX_EVENT_DATA_LEN` bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventText(Bytes);

impl EventText {
    /// `None` if `text` is empty or longer than `MAX_EVENT_DATA_LEN` bytes.
    pub fn new(text: &str) -> Option<Self> {
        if text.is_empty() {
            return None;
        }
        return Bytes::new(text.as_bytes()).map(Self);
    }

    pub fn as_str(&self) -> &str {
        return core::str::from_utf8(self.0.as_slice()).expect("only built from a str");
    }
}

impl core::fmt::Debug for EventText {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        return core::fmt::Debug::fmt(self.as_str(), f);
    }
}

/// The raw data of an `UnknownEvent`: 0 to `MAX_EVENT_DATA_LEN` bytes.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventData(Bytes);

impl EventData {
    /// `None` if `data` is longer than `MAX_EVENT_DATA_LEN` bytes.
    pub fn new(data: &[u8]) -> Option<Self> {
        return Bytes::new(data).map(Self);
    }

    pub fn as_bytes(&self) -> &[u8] {
        return self.0.as_slice();
    }
}

impl core::fmt::Debug for EventData {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        return core::fmt::Debug::fmt(self.as_bytes(), f);
    }
}

/// A field inside an event's data.
trait Field: Sized {
    fn read(r: &mut Reader) -> Result<Self, MessageError>;

    fn write(self, w: &mut Writer);
}

impl Field for u16 {
    fn read(r: &mut Reader) -> Result<Self, MessageError> {
        return r.u16();
    }

    fn write(self, w: &mut Writer) {
        w.u16(self);
    }
}

impl Field for u32 {
    fn read(r: &mut Reader) -> Result<Self, MessageError> {
        return r.u32();
    }

    fn write(self, w: &mut Writer) {
        w.u32(self);
    }
}

impl Field for FailsafeCause {
    fn read(r: &mut Reader) -> Result<Self, MessageError> {
        return Ok(Self::from(r.u8()?));
    }

    fn write(self, w: &mut Writer) {
        w.u8(u8::from(self));
    }
}

/// A unit struct for an event without data, so it is written `Armed` rather than `Armed {}`.
macro_rules! event_struct {
    ($(#[$meta:meta])* $name:ident {}) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $name;
    };
    ($(#[$meta:meta])* $name:ident { $($(#[$field_meta:meta])* $field:ident: $ty:ty,)+ }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct $name {
            $($(#[$field_meta])* pub $field: $ty,)+
        }
    };
}

/// Defines one struct per known event (named like its `EventCode`), and `Event` over all of them.
macro_rules! events {
    ($($(#[$meta:meta])* $name:ident = $level:ident { $($(#[$field_meta:meta])* $field:ident: $ty:ty,)* })*) => {
        $(
            event_struct!($(#[$meta])* $name { $($(#[$field_meta])* $field: $ty,)* });

            impl EventKind for $name {
                const CODE: EventCode = EventCode::$name;
                const LEVEL: EventLevel = EventLevel::$level;
            }
        )*

        #[allow(clippy::large_enum_variant)] // owned messages are as big as the biggest one (protocol.md §7.1)
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Event {
            $($name($name),)*
            Text(Text),
            Unknown(UnknownEvent),
        }

        impl Event {
            /// `None` for `Text`, which has Code 0.
            pub fn code(&self) -> Option<EventCode> {
                return match self {
                    $(Event::$name(_) => Some($name::CODE),)*
                    Event::Text(_) => None,
                    Event::Unknown(event) => Some(EventCode::Unknown(event.code)),
                };
            }

            pub fn level(&self) -> EventLevel {
                return match self {
                    $(Event::$name(_) => $name::LEVEL,)*
                    Event::Text(text) => text.level,
                    Event::Unknown(event) => event.level,
                };
            }

            pub(crate) fn write(&self, w: &mut Writer) {
                w.u8(u8::from(self.level()));
                w.u16(self.code().map_or(0, |code| return NonZeroU16::from(code).get()));
                match self {
                    $(Event::$name(_event) => { $(_event.$field.write(w);)* })*
                    Event::Text(text) => w.put(text.text.as_str().as_bytes()),
                    Event::Unknown(event) => w.put(event.data.as_bytes()),
                }
            }

            pub(crate) fn read(r: &mut Reader) -> Result<Self, MessageError> {
                let level = EventLevel::from(r.u8()?);
                let code = NonZeroU16::new(r.u16()?).map(EventCode::from);
                let event = match code {
                    None => Event::Text(Text {
                        level,
                        text: EventText::new(core::str::from_utf8(r.rest()).map_err(invalid)?)
                            .ok_or(MessageError::InvalidValue)?,
                    }),
                    Some(EventCode::Unknown(code)) => Event::Unknown(UnknownEvent {
                        level,
                        code,
                        data: EventData::new(r.rest()).ok_or(MessageError::WrongLength)?,
                    }),
                    $(Some(EventCode::$name) => Event::$name($name { $($field: <$ty as Field>::read(r)?,)* }),)*
                };
                if event.level() != level {
                    return Err(MessageError::InvalidValue); // a known event always has its own level
                }
                return Ok(event);
            }
        }
    };
}

events! {
    Armed = Info {}
    Disarmed = Info {}
    LinkLost = Warning {}
    LinkRestored = Info {}
    BatteryLow = Warning {
        /// Whole pack.
        millivolts: u16,
    }
    BatteryCritical = Error {
        /// Whole pack.
        millivolts: u16,
    }
    /// An automatic safety action took over.
    FailsafeTriggered = Error {
        cause: FailsafeCause,
    }
    /// The flight loop missed its deadline.
    LoopOverrun = Error {
        /// How late the loop run finished, in µs.
        late_us: u32,
    }
    ImuError = Error {}
}
