//! One-byte codes from protocol.md §5.

/// A byte that is not one of the enum's codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnknownCode(pub u8);

macro_rules! wire_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $value:literal,)* }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u8)]
        pub enum $name {
            $($variant = $value,)*
        }

        impl TryFrom<u8> for $name {
            type Error = UnknownCode;

            fn try_from(value: u8) -> Result<Self, UnknownCode> {
                return match value {
                    $($value => Ok(Self::$variant),)*
                    _ => Err(UnknownCode(value)),
                };
            }
        }

        impl From<$name> for u8 {
            fn from(code: $name) -> u8 {
                return code as u8;
            }
        }
    };
}

/// Like `wire_enum!`, plus `Unknown(byte)` for codes the receiver must still see, to answer them.
macro_rules! open_wire_enum {
    ($(#[$meta:meta])* $name:ident { $($variant:ident = $value:literal,)* }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name {
            $($variant,)*
            Unknown(u8), // only produced by decoding, never holds a known code
        }

        impl From<u8> for $name {
            fn from(value: u8) -> Self {
                return match value {
                    $($value => Self::$variant,)*
                    other => Self::Unknown(other),
                };
            }
        }

        impl From<$name> for u8 {
            fn from(code: $name) -> u8 {
                return match code {
                    $($name::$variant => $value,)*
                    $name::Unknown(value) => value,
                };
            }
        }
    };
}

open_wire_enum!(MessageType {
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
    ListLogs = 0x70,
    LogList = 0x71,
    ReadLog = 0x72,
    LogChunk = 0x73,
});

wire_enum!(HelloResult {
    Ok = 0,
    VersionMismatch = 1,
    OptionNotSupported = 2,
});

wire_enum!(LinkTestDirection {
    DroneSends = 0,
    ClientSends = 1,
});

wire_enum!(
    /// Status of a Reply.
    Status {
        Ok = 0,
        Refused = 1,
        UnknownType = 2,
        Malformed = 3,
        NotSupported = 4,
        Busy = 5,
    }
);

wire_enum!(EventLevel {
    Debug = 0,
    Info = 1,
    Warning = 2,
    Error = 3,
});

open_wire_enum!(
    /// Units per stream are in protocol.md §5.6.
    StreamId {
        Status = 1,
        Battery = 2,
        Attitude = 3,
        Height = 4,
        RawImu = 5,
        Gps = 6, // reserved until GPS is fitted
        LinkQuality = 7,
    }
);
