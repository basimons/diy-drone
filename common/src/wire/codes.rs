//! One-byte codes from protocol.md §5.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UnknownCode<T = u8>(pub T);

/// Defines a code enum stored as `$repr` (`u8` or `u16`), with conversions to and from it.
/// Plain: `TryFrom<$repr>` rejects unknown values with `UnknownCode`.
/// `with Unknown`: adds an `Unknown(value)` variant and an infallible `From<$repr>`, for codes the drone must
/// still see to answer them.
macro_rules! wire_enum {
    (@decode $name:ident: $repr:ident { $($variant:ident = $value:literal,)* }) => {
        impl TryFrom<$repr> for $name {
            type Error = UnknownCode<$repr>;

            fn try_from(value: $repr) -> Result<Self, UnknownCode<$repr>> {
                return match value {
                    $($value => Ok(Self::$variant),)*
                    _ => Err(UnknownCode(value)),
                };
            }
        }
    };
    (@decode $name:ident: $repr:ident $unknown:ident { $($variant:ident = $value:literal,)* }) => {
        impl From<$repr> for $name {
            fn from(value: $repr) -> Self {
                return match value {
                    $($value => Self::$variant,)*
                    other => Self::$unknown(other),
                };
            }
        }
    };
    ($(#[$meta:meta])* $name:ident: $repr:ident $(with $unknown:ident)? { $($variant:ident = $value:literal,)* }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum $name {
            $($variant,)*
            $($unknown($repr),)? // only produced by decoding, never holds a known code
        }

        impl From<$name> for $repr {
            fn from(code: $name) -> $repr {
                return match code {
                    $($name::$variant => $value,)*
                    $($name::$unknown(value) => value,)?
                };
            }
        }

        wire_enum!(@decode $name: $repr $($unknown)? { $($variant = $value,)* });
    };
}

wire_enum!(MessageType: u8 with Unknown {
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
    /// Status of a Reply.
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
    StreamId: u8 with Unknown {
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
    /// Why a Reply has its Status (protocol.md §5.4). The high byte groups them. On the wire 0 means "no
    /// reason"; that is `None` in `Reply`, never a code.
    ReasonCode: u16 {
        ThrottleNotLow = 0x0101, // Arm: throttle must be at the bottom
        ImuNotReady = 0x0102,
        BatteryLow = 0x0103,
        NotArmed = 0x0201,
        NotFlying = 0x0202,
        TooLow = 0x0203, // e.g. Flip close to the ground
        NotAllowedInSteadyState = 0x0204,
        NoGps = 0x0301, // GoHome
        UnknownStream = 0x0401,
        RateNotPossible = 0x0402,
        UnknownSettingsGroup = 0x0403,
        ValueOutOfRange = 0x0404,
        UnknownLog = 0x0405,
        OffsetPastEnd = 0x0406, // ReadLog beyond the end of the log
    }
);

wire_enum!(
    /// What an Event is about (protocol.md §5.4). The high byte groups them. On the wire 0 means "no code";
    /// that is `None` in `Event`, never a code.
    EventCode: u16 {
        Armed = 0x0101,
        Disarmed = 0x0102,
        LinkLost = 0x0201,
        LinkRestored = 0x0202,
        BatteryLow = 0x0301,
        BatteryCritical = 0x0302,
        FailsafeTriggered = 0x0401, // an automatic safety action took over
        LoopOverrun = 0x0402,       // the flight loop missed its deadline
        ImuError = 0x0403,
    }
);

/// A type byte that is not a known message type. Can't hold a known byte, unlike `MessageType::Unknown`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct UnknownType(u8);

impl UnknownType {
    pub fn from_byte(byte: u8) -> Option<Self> {
        return match MessageType::from(byte) {
            MessageType::Unknown(_) => Some(Self(byte)),
            _ => None,
        };
    }

    pub fn byte(self) -> u8 {
        return self.0;
    }
}

/// A known message type that is not a request, so a Reply can't answer it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NotARequest(pub MessageType);

/// Defines `RequestType` as the listed `MessageType`s plus `Unknown`, with conversions both ways.
macro_rules! request_types {
    ($($name:ident,)*) => {
        /// What a Reply can answer: a request (protocol.md §5.0), or a type the drone doesn't know.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum RequestType {
            $($name,)*
            Unknown(UnknownType),
        }

        impl From<RequestType> for MessageType {
            fn from(request: RequestType) -> Self {
                return match request {
                    $(RequestType::$name => MessageType::$name,)*
                    RequestType::Unknown(unknown) => MessageType::Unknown(unknown.0),
                };
            }
        }

        impl TryFrom<MessageType> for RequestType {
            type Error = NotARequest;

            fn try_from(message_type: MessageType) -> Result<Self, NotARequest> {
                // Re-reading the byte turns a hand-made `MessageType::Unknown(0x20)` into `Arm`.
                return match MessageType::from(u8::from(message_type)) {
                    $(MessageType::$name => Ok(RequestType::$name),)*
                    MessageType::Unknown(byte) => {
                        debug_assert!(UnknownType::from_byte(byte).is_some(), "{byte:#04x} is known");
                        Ok(RequestType::Unknown(UnknownType(byte)))
                    }
                    other => Err(NotARequest(other)),
                };
            }
        }
    };
}

request_types!(
    LinkTest,
    Arm,
    Disarm,
    TakeOff,
    Land,
    GoHome,
    Flip,
    SteadyState,
    ExitSteadyState,
    SetSettings,
    GetSettings,
    RequestOnce,
    Subscribe,
    ListLogs,
    ReadLog,
);
