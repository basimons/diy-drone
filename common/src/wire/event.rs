//! Event (protocol.md §5.4): the drone tells the client something happened. One type per event, holding only
//! the data that event has; the level and code of an event are fixed by its type.

use crate::wire::codes::{EventLevel, FailsafeCause};
use crate::wire::message::MessageError;
use crate::wire::packet::MAX_PAYLOAD_LEN;
use crate::wire::payload::{Reader, Wire, Writer, impl_wire, invalid};

/// The rest of the payload after Level and Code.
pub const MAX_EVENT_TEXT_LEN: usize = MAX_PAYLOAD_LEN - 3;

/// What every event type has: its code and level on the wire.
pub trait EventKind {
    const CODE: u16;
    const LEVEL: EventLevel;
}

/// Free text from the drone, sent with Code 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Text {
    pub level: EventLevel,
    pub text: EventText,
}

/// UTF-8 text of 1 to `MAX_EVENT_TEXT_LEN` bytes, owned (protocol.md §7.1).
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventText {
    bytes: [u8; MAX_EVENT_TEXT_LEN], // zero after `len`, so the derived `PartialEq` compares the text only
    len: usize,
}

impl EventText {
    /// `None` if `text` is empty or longer than `MAX_EVENT_TEXT_LEN` bytes.
    pub fn new(text: &str) -> Option<Self> {
        if text.is_empty() || text.len() > MAX_EVENT_TEXT_LEN {
            return None;
        }
        return Some(Self {
            bytes: core::array::from_fn(|i| return text.as_bytes().get(i).copied().unwrap_or(0)),
            len: text.len(),
        });
    }

    pub fn as_str(&self) -> &str {
        debug_assert!((1..=MAX_EVENT_TEXT_LEN).contains(&self.len), "length {}", self.len);
        debug_assert!(
            self.bytes[self.len..].iter().all(|&b| return b == 0),
            "PartialEq needs zero padding"
        );
        return core::str::from_utf8(&self.bytes[..self.len]).expect("only built from a str");
    }
}

impl core::fmt::Debug for EventText {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        return core::fmt::Debug::fmt(self.as_str(), f);
    }
}

/// Takes the rest of the payload.
impl Wire for EventText {
    fn read(r: &mut Reader) -> Result<Self, MessageError> {
        let text = core::str::from_utf8(r.rest()).map_err(invalid)?;
        return EventText::new(text).ok_or(MessageError::InvalidValue);
    }

    fn write(&self, w: &mut Writer) {
        w.put(self.as_str().as_bytes());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Armed;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Disarmed;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LinkLost;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LinkRestored;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BatteryLow {
    pub millivolts: u16, // whole pack
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BatteryCritical {
    pub millivolts: u16, // whole pack
}

/// An automatic safety action took over.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FailsafeTriggered {
    pub cause: FailsafeCause,
}

/// The flight loop missed its deadline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LoopOverrun {
    pub late_us: u32, // how late the loop run finished
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImuError;

impl_wire!(Armed {});
impl_wire!(Disarmed {});
impl_wire!(LinkLost {});
impl_wire!(LinkRestored {});
impl_wire!(BatteryLow { millivolts });
impl_wire!(BatteryCritical { millivolts });
impl_wire!(FailsafeTriggered { cause });
impl_wire!(LoopOverrun { late_us });
impl_wire!(ImuError {});

/// Defines `Event` from one list of event types with their code and level.
macro_rules! events {
    ($($name:ident = $code:literal, $level:ident;)*) => {
        const _: () = assert!($($code != 0 &&)* true, "Code 0 is Text");

        $(
            impl EventKind for $name {
                const CODE: u16 = $code;
                const LEVEL: EventLevel = EventLevel::$level;
            }
        )*

        #[allow(clippy::large_enum_variant)] // owned messages are as big as the biggest one (protocol.md §7.1)
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Event {
            $($name($name),)*
            Text(Text),
        }

        impl Event {
            pub fn level(&self) -> EventLevel {
                return match self {
                    $(Event::$name(_) => $name::LEVEL,)*
                    Event::Text(text) => text.level,
                };
            }

            fn code(&self) -> u16 {
                return match self {
                    $(Event::$name(_) => $name::CODE,)*
                    Event::Text(_) => 0,
                };
            }
        }

        impl Wire for Event {
            fn read(r: &mut Reader) -> Result<Self, MessageError> {
                let level = EventLevel::read(r)?;
                let event = match u16::read(r)? {
                    0 => Event::Text(Text { level, text: EventText::read(r)? }),
                    $($code => Event::$name($name::read(r)?),)*
                    code => return Err(MessageError::UnknownCode { kind: "EventCode", value: code }),
                };
                if event.level() != level {
                    return Err(MessageError::InvalidValue); // an event other than Text always has its own level
                }
                return Ok(event);
            }

            fn write(&self, w: &mut Writer) {
                self.level().write(w);
                self.code().write(w);
                match self {
                    $(Event::$name(event) => event.write(w),)*
                    Event::Text(text) => text.text.write(w),
                }
            }
        }
    };
}

events! {
    Armed = 0x0101, Info;
    Disarmed = 0x0102, Info;
    LinkLost = 0x0201, Warning;
    LinkRestored = 0x0202, Info;
    BatteryLow = 0x0301, Warning;
    BatteryCritical = 0x0302, Error;
    FailsafeTriggered = 0x0401, Error;
    LoopOverrun = 0x0402, Error;
    ImuError = 0x0403, Error;
}
