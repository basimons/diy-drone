/// Defines every event from one list: its struct, code and level, and `Event` over all of them plus `Text`.
macro_rules! events {
    ($($(#[$meta:meta])* $name:ident = $code:literal, $level:ident { $($fields:tt)* },)*) => {
        const _: () = assert!($($code != 0 &&)* true, "Code 0 is Text");

        $(
            $crate::wire::macros::wire_struct!($(#[$meta])* $name { $($fields)* });

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
pub(crate) use events;
