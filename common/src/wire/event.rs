//! Event (protocol.md §5.4): the drone tells the client something happened. One type per event, holding only
//! the data that event has; the level and code of an event are fixed by its type.

use core::fmt::Write;

use crate::wire::macros::{code, events, wire_struct};
use crate::wire::message::MessageError;
use crate::wire::packet::MAX_PAYLOAD_LEN;
use crate::wire::payload::{Reader, Wire, Writer, invalid};

pub use crate::wire::message::Event;

/// The rest of the payload after Event ID, Level and Code.
pub const MAX_EVENT_TEXT_LEN: usize = MAX_PAYLOAD_LEN - 5;
const _: () = assert!(
    MAX_EVENT_TEXT_LEN <= u8::MAX as usize,
    "EventText stores its length in a u8"
);

wire_struct!(
    /// Numbers the drone's events, so the client shows a resent one only once (protocol.md §5.0a). Every value is
    /// valid: the drone counts up with `wrapping_add`.
    EventId(pub u16)
);

code!(pub EventLevel: u8 {
    Debug = 0,
    Info = 1,
    Warning = 2,
    Error = 3,
});

code!(
    /// Why a failsafe took over (drone-design.md §4).
    pub FailsafeCause: u8 {
        LinkLoss = 1,
        ClientFrozen = 2, // the link is alive but the client stopped updating (protocol.md §6 rule 5)
        LowBattery = 3,
        LoopOverrun = 4,
    }
);

/// An event with its own type: its Code and its fixed Level on the wire. Text chooses its own level instead.
pub trait FixedEvent {
    const CODE: u16;
    const LEVEL: EventLevel;
}

events! {
    Armed = 0x0101, Info {},
    Disarmed = 0x0102, Info {},
    LinkLost = 0x0201, Warning {},
    LinkRestored = 0x0202, Info {},
    BatteryLow = 0x0301, Warning {
        millivolts: u16, // whole pack
    },
    BatteryCritical = 0x0302, Error {
        millivolts: u16, // whole pack
    },
    /// An automatic safety action took over.
    FailsafeTriggered = 0x0401, Error { cause: FailsafeCause },
    /// The flight loop missed its deadline.
    LoopOverrun = 0x0402, Error {
        late_us: u32, // how late the loop run finished
    },
    ImuError = 0x0403, Error {},
}

/// Free text from the drone, sent with Code 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Text {
    pub level: EventLevel,
    pub text: EventText,
}

/// A string of 1 to `MAX_EVENT_TEXT_LEN` bytes of UTF-8 that needs no heap. Use it like a `&str`; build it with
/// `new` or, for numbers in the text, `format`.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct EventText {
    bytes: [u8; MAX_EVENT_TEXT_LEN], // zero after `len`, so the derived `PartialEq` compares the text only
    len: u8,                         // a u8 holds MAX_EVENT_TEXT_LEN and keeps every Message smaller
}

impl EventText {
    /// `None` if `text` is empty or longer than `MAX_EVENT_TEXT_LEN` bytes.
    pub fn new(text: &str) -> Option<Self> {
        return Self::format(format_args!("{text}"));
    }

    /// Like `format!`: `EventText::format(format_args!("cell {n} low"))`. `None` if the result is empty or longer
    /// than `MAX_EVENT_TEXT_LEN` bytes.
    pub fn format(args: core::fmt::Arguments) -> Option<Self> {
        let mut buffer = Buffer(Self {
            bytes: [0; MAX_EVENT_TEXT_LEN],
            len: 0,
        });
        buffer.write_fmt(args).ok()?;
        return if buffer.0.len == 0 { None } else { Some(buffer.0) };
    }
}

/// Builds an `EventText` in `format`; private, so a finished text can't be appended to.
struct Buffer(EventText);

impl Write for Buffer {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let start = usize::from(self.0.len);
        let end = start + s.len();
        if end > MAX_EVENT_TEXT_LEN {
            return Err(core::fmt::Error);
        }
        self.0.bytes[start..end].copy_from_slice(s.as_bytes());
        self.0.len = u8::try_from(end).expect("MAX_EVENT_TEXT_LEN fits in a u8");
        return Ok(());
    }
}

impl core::ops::Deref for EventText {
    type Target = str;

    fn deref(&self) -> &str {
        let len = usize::from(self.len);
        debug_assert!((1..=MAX_EVENT_TEXT_LEN).contains(&len), "length {len}");
        debug_assert!(
            self.bytes[len..].iter().all(|&b| return b == 0),
            "PartialEq needs zero padding"
        );
        return core::str::from_utf8(&self.bytes[..len]).expect("only built from a str");
    }
}

impl core::fmt::Display for EventText {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        return f.pad(self);
    }
}

impl core::fmt::Debug for EventText {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        return core::fmt::Debug::fmt(&**self, f);
    }
}

/// Takes the rest of the payload.
impl Wire for EventText {
    fn read(r: &mut Reader) -> Result<Self, MessageError> {
        let text = core::str::from_utf8(r.rest()).map_err(invalid)?;
        return EventText::new(text).ok_or(MessageError::InvalidValue);
    }

    fn write(&self, w: &mut Writer) {
        w.put(self.as_bytes());
    }
}
