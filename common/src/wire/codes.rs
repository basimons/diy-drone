//! One-byte codes from protocol.md §5.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnknownCode(pub u8);

/// Defines a one-byte code enum with `ALL` (every code, in protocol order) and conversions to and from `u8`.
/// Plain: `TryFrom<u8>` rejects unknown bytes with `UnknownCode`.
/// `with Unknown`: adds an `Unknown(byte)` variant and an infallible `From<u8>`, for codes the drone must still
/// see to answer them.
macro_rules! wire_enum {
    (@decode $name:ident { $($variant:ident = $value:literal,)* }) => {
        impl TryFrom<u8> for $name {
            type Error = UnknownCode;

            fn try_from(byte: u8) -> Result<Self, UnknownCode> {
                return match byte {
                    $($value => Ok(Self::$variant),)*
                    _ => Err(UnknownCode(byte)),
                };
            }
        }
    };
    (@decode $name:ident $unknown:ident { $($variant:ident = $value:literal,)* }) => {
        impl From<u8> for $name {
            fn from(byte: u8) -> Self {
                return match byte {
                    $($value => Self::$variant,)*
                    other => Self::$unknown(other),
                };
            }
        }
    };
    ($(#[$meta:meta])* $name:ident $(with $unknown:ident)? { $($variant:ident = $value:literal,)* }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name {
            $($variant,)*
            $($unknown(u8),)? // only produced by decoding, never holds a known code
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$(Self::$variant,)*];
        }

        impl From<$name> for u8 {
            fn from(code: $name) -> u8 {
                return match code {
                    $($name::$variant => $value,)*
                    $($name::$unknown(byte) => byte,)?
                };
            }
        }

        wire_enum!(@decode $name $($unknown)? { $($variant = $value,)* });
    };
}

wire_enum!(MessageType with Unknown {
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

wire_enum!(
    /// Units per stream are in protocol.md §5.6.
    StreamId with Unknown {
        Status = 1,
        Battery = 2,
        Attitude = 3,
        Height = 4,
        RawImu = 5,
        Gps = 6, // reserved until GPS is fitted
        LinkQuality = 7,
    }
);
