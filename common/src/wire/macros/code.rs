/// Defines an enum stored on the wire as a number of type `$repr`. A number that is none of its values doesn't
/// decode (`MessageError::UnknownCode`).
macro_rules! code {
    ($(#[$meta:meta])* $vis:vis $name:ident: $repr:ident { $($variant:ident = $value:literal),* $(,)? }) => {
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
