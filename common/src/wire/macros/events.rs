/// Defines every event from one list: its struct, code and level, and `EventKind` over all of them plus `Text`.
macro_rules! events {
    ($($(#[$meta:meta])* $name:ident = $code:literal, $level:ident { $($fields:tt)* },)*) => {
        const _: () = assert!($($code != 0 &&)* true, "Code 0 is Text");

        $(
            $crate::wire::macros::wire_struct!($(#[$meta])* $name { $($fields)* });

            impl FixedEvent for $name {
                const CODE: u16 = $code;
                const LEVEL: EventLevel = EventLevel::$level;
            }
        )*

        #[allow(clippy::large_enum_variant)] // owned messages are as big as the biggest one (protocol.md §7.1)
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum EventKind {
            $($name($name),)*
            Text(Text),
        }

        impl EventKind {
            pub fn level(&self) -> EventLevel {
                return match self {
                    $(EventKind::$name(_) => $name::LEVEL,)*
                    EventKind::Text(text) => text.level,
                };
            }

            fn code(&self) -> u16 {
                return match self {
                    $(EventKind::$name(_) => $name::CODE,)*
                    EventKind::Text(_) => 0,
                };
            }
        }

        /// Level, Code and Data.
        impl Wire for EventKind {
            fn read(r: &mut Reader) -> Result<Self, MessageError> {
                let level = EventLevel::read(r)?;
                let event = match u16::read(r)? {
                    0 => EventKind::Text(Text { level, text: EventText::read(r)? }),
                    $($code => EventKind::$name($name::read(r)?),)*
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
                    $(EventKind::$name(event) => event.write(w),)*
                    EventKind::Text(text) => text.text.write(w),
                }
            }
        }
    };
}
pub(crate) use events;
