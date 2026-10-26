//! The codes of protocol.md §5 that messages carry. Each holds only the values the protocol defines; a number that
//! is none of them fails decoding with `MessageError::UnknownCode`.

use core::convert::Infallible;

use crate::wire::message::MessageError;
use crate::wire::payload::Code;

/// Defines a code enum and its wire value `$repr`.
macro_rules! code {
    ($(#[$meta:meta])* $vis:vis $name:ident: $repr:ident { $($variant:ident = $value:literal,)* }) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[repr($repr)]
        $vis enum $name {
            $($variant = $value,)*
        }

        impl $crate::wire::payload::Code for $name {
            type Repr = $repr;

            fn from_repr(value: $repr) -> Result<Self, $crate::wire::message::MessageError> {
                return match value {
                    $($value => Ok(Self::$variant),)*
                    _ => Err($crate::wire::message::MessageError::UnknownCode {
                        kind: stringify!($name),
                        value: value.into(),
                    }),
                };
            }

            fn repr(self) -> $repr {
                return self as $repr;
            }
        }
    };
}
pub(crate) use code;

/// The refusal of requests that are never refused (e.g. Disarm): `Failure::Refused` can't be built for them.
impl Code for Infallible {
    type Repr = u16;

    fn from_repr(_: u16) -> Result<Self, MessageError> {
        return Err(MessageError::InvalidValue);
    }

    fn repr(self) -> u16 {
        match self {} // no value exists, so there is nothing to `return`
    }
}

code!(
    /// Never changes between protocol versions, like the rest of Hello and HelloReply (protocol.md §5.1).
    pub HelloResult: u8 {
        Ok = 0,
        VersionMismatch = 1,
        OptionNotSupported = 2,
    }
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
        ClientFrozen = 2, // the client's Sticks stopped changing
        LowBattery = 3,
        LoopOverrun = 4,
    }
);

// Why a request was refused, numbered per request (protocol.md §5.4). Never 0: Reason 0 means "no reason".

code!(pub ArmRefusal: u16 {
    ThrottleNotLow = 1,
    ImuNotReady = 2,
    BatteryLow = 3,
});

code!(pub TakeOffRefusal: u16 {
    NotArmed = 1,
    BatteryLow = 2,
});

code!(pub LandRefusal: u16 { NotFlying = 1, });

code!(pub FlipRefusal: u16 {
    NotArmed = 1,
    NotFlying = 2,
    TooLow = 3,
});

code!(pub SteadyStateRefusal: u16 { NotFlying = 1, });
