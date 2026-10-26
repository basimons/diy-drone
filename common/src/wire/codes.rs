//! The codes of protocol.md §5. Every code type keeps values it doesn't know as `Unknown`, so a newer peer's
//! codes still arrive and the caller decides what to do with them (e.g. log them).

use core::convert::Infallible;
use core::fmt::Debug;
use core::hash::Hash;
use core::marker::PhantomData;
use core::num::NonZeroU16;

/// A code type, with the wire value it is stored as.
pub trait Code: Copy {
    type Repr: Copy + Eq + Hash + Debug;

    fn is_known(value: Self::Repr) -> bool;
}

/// A value that is not one of `E`'s codes. Can't hold a known value, so every code has one representation.
pub struct Unknown<E: Code> {
    value: E::Repr,
    code: PhantomData<E>,
}

impl<E: Code> Unknown<E> {
    /// `None` if `value` is a known code.
    pub fn new(value: E::Repr) -> Option<Self> {
        return if E::is_known(value) {
            None
        } else {
            Some(Self::unchecked(value))
        };
    }

    /// Only for `From`, after none of the known codes matched.
    fn unchecked(value: E::Repr) -> Self {
        return Self {
            value,
            code: PhantomData,
        };
    }

    pub fn value(self) -> E::Repr {
        return self.value;
    }
}

// Written out because derives would demand the bounds of `E` instead of `E::Repr`.
impl<E: Code> Clone for Unknown<E> {
    fn clone(&self) -> Self {
        return *self;
    }
}

impl<E: Code> Copy for Unknown<E> {}

impl<E: Code> PartialEq for Unknown<E> {
    fn eq(&self, other: &Self) -> bool {
        return self.value == other.value;
    }
}

impl<E: Code> Eq for Unknown<E> {}

impl<E: Code> Hash for Unknown<E> {
    fn hash<H: core::hash::Hasher>(&self, state: &mut H) {
        self.value.hash(state);
    }
}

impl<E: Code> Debug for Unknown<E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        return write!(f, "Unknown({:?})", self.value);
    }
}

/// Why a request was refused. The reason codes are numbered per request (protocol.md §5.4) and are never 0,
/// because Reason 0 means "no reason".
pub trait Refusal: Copy {
    fn from_reason(reason: u16) -> Option<Self>;

    fn reason(self) -> u16;
}

/// For requests that are never refused (e.g. Disarm): `Failure::Refused` can't be built for them.
impl Refusal for Infallible {
    fn from_reason(_: u16) -> Option<Self> {
        return None;
    }

    fn reason(self) -> u16 {
        match self {}
    }
}

/// Defines a code enum with an `Unknown` variant and conversions to and from its wire value `$repr`.
/// `$repr` is `u8`, or `NonZeroU16` for two-byte codes where 0 means "none" on the wire.
macro_rules! wire_enum {
    (@value u8, $value:literal) => {
        $value
    };
    (@value NonZeroU16, $value:literal) => {
        const { NonZeroU16::new($value).expect("0 means none") }
    };
    (@match u8, $value:expr) => {
        $value
    };
    (@match NonZeroU16, $value:expr) => {
        $value.get()
    };
    ($(#[$meta:meta])* $name:ident: $repr:ident { $($variant:ident = $value:literal,)* }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $($variant,)*
            Unknown(Unknown<$name>),
        }

        impl Code for $name {
            type Repr = $repr;

            fn is_known(value: $repr) -> bool {
                return !matches!(Self::from(value), Self::Unknown(_));
            }
        }

        impl From<$repr> for $name {
            fn from(value: $repr) -> Self {
                return match wire_enum!(@match $repr, value) {
                    $($value => Self::$variant,)*
                    _ => Self::Unknown(Unknown::unchecked(value)),
                };
            }
        }

        impl From<$name> for $repr {
            fn from(code: $name) -> $repr {
                return match code {
                    $($name::$variant => wire_enum!(@value $repr, $value),)*
                    $name::Unknown(unknown) => unknown.value,
                };
            }
        }
    };
}

/// Defines a refusal enum: a two-byte code that is also a `Refusal`.
macro_rules! refusal {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $value:literal,)* }) => {
        wire_enum!($(#[$meta])* $name: NonZeroU16 { $($variant = $value,)* });

        impl Refusal for $name {
            fn from_reason(reason: u16) -> Option<Self> {
                return NonZeroU16::new(reason).map(Self::from);
            }

            fn reason(self) -> u16 {
                return NonZeroU16::from(self).get();
            }
        }
    };
}

wire_enum!(MessageType: u8 {
    Hello = 0x01,
    HelloReply = 0x02,
    Ping = 0x03,
    Pong = 0x04,
    LinkTest = 0x05,
    LinkTestData = 0x06,
    LinkTestResult = 0x07,
    Sticks = 0x10,
    Arm = 0x20,
    Disarm = 0x21,
    TakeOff = 0x22,
    Land = 0x23,
    GoHome = 0x24,
    Flip = 0x25,
    SteadyState = 0x26,
    ExitSteadyState = 0x27,
    Reply = 0x40,
    Event = 0x41,
    SetSettings = 0x50,
    GetSettings = 0x51,
    Settings = 0x52,
    RequestOnce = 0x60,
    Subscribe = 0x61,
    StreamData = 0x62,
    StreamUpdate = 0x63,
    ListLogs = 0x70,
    LogList = 0x71,
    ReadLog = 0x72,
    LogChunk = 0x73,
});

wire_enum!(HelloResult: u8 {
    Ok = 0,
    VersionMismatch = 1,
    OptionNotSupported = 2,
});

wire_enum!(LinkTestDirection: u8 {
    DroneSends = 0,
    ClientSends = 1,
});

wire_enum!(
    /// Status byte of a Reply. In Rust a Reply holds an `Answer` instead, which can't combine them wrongly.
    Status: u8 {
        Ok = 0,
        Refused = 1,
        UnknownType = 2,
        Malformed = 3,
        NotSupported = 4,
        Busy = 5,
    }
);

wire_enum!(EventLevel: u8 {
    Debug = 0,
    Info = 1,
    Warning = 2,
    Error = 3,
});

wire_enum!(
    /// Units per stream are in protocol.md §5.6.
    StreamId: u8 {
        Status = 1,
        Battery = 2,
        Attitude = 3,
        Height = 4,
        RawImu = 5,
        Gps = 6, // reserved until GPS is fitted
        LinkQuality = 7,
    }
);

wire_enum!(
    /// What an Event is about (protocol.md §5.4). The high byte groups them. "No code" is `None` in `Event`.
    EventCode: NonZeroU16 {
        Armed = 0x0101,
        Disarmed = 0x0102,
        LinkLost = 0x0201,
        LinkRestored = 0x0202,
        BatteryLow = 0x0301,
        BatteryCritical = 0x0302,
        FailsafeTriggered = 0x0401,
        LoopOverrun = 0x0402,
        ImuError = 0x0403,
    }
);

wire_enum!(
    /// Why a failsafe took over (drone-design.md §4). 0 is never used.
    FailsafeCause: u8 {
        LinkLoss = 1,
        ClientFrozen = 2, // the client's Sticks stopped changing
        LowBattery = 3,
        LoopOverrun = 4,
    }
);

refusal!(ArmRefusal {
    ThrottleNotLow = 1,
    ImuNotReady = 2,
    BatteryLow = 3,
});

refusal!(TakeOffRefusal {
    NotArmed = 1,
    BatteryLow = 2,
});

refusal!(LandRefusal { NotFlying = 1, });

refusal!(FlipRefusal {
    NotArmed = 1,
    NotFlying = 2,
    TooLow = 3,
});

refusal!(SteadyStateRefusal { NotFlying = 1, });

refusal!(LinkTestRefusal {
    NotAllowedNow = 1, // e.g. while flying (protocol.md §5.1a)
});

refusal!(SetSettingsRefusal {
    UnknownGroup = 1,
    ValueOutOfRange = 2,
});

refusal!(GetSettingsRefusal { UnknownGroup = 1, });

refusal!(RequestOnceRefusal { UnknownStream = 1, });

refusal!(SubscribeRefusal {
    UnknownStream = 1,
    RateNotPossible = 2,
});

refusal!(ReadLogRefusal {
    UnknownLog = 1,
    OffsetPastEnd = 2,
});
