/// Defines every message from one list: its struct (unless it is defined elsewhere, like `Reply`), its type byte and
/// `Message` over all of them. Every request is a command so far (protocol.md §5.3); for each one it also defines its
/// refusal codes, its `Answer` variant, its `RequestType` and its `Command` variant. A command's struct is its Request
/// ID, its Sent At, then the fields listed, which are the command's own and shared with its `Command` variant. Only doc
/// comments go on a command, since they are copied to both. `expires` marks a command the drone refuses as too late;
/// its `refused` must include `TooLate`. `refused` lists the `Reason`s it may be refused for, as its own enum; one
/// without `refused` is never refused. Requests that are not commands (LinkTest, settings, logs) will be a second
/// list in this same call: `RequestType`, `Answer` and `TypeKind` must each be generated once, over all requests.
macro_rules! messages {
    (@refusal) => { ::core::convert::Infallible };
    (@refusal $refusal:ident) => { $refusal };
    (@struct $(#[$meta:meta])* $name:ident) => {}; // defined elsewhere
    (@struct $(#[$meta:meta])* $name:ident { $($fields:tt)* }) => {
        $crate::wire::macros::wire_struct!($(#[$meta])* $name { $($fields)* });
    };
    (@command_struct [$(#[$meta:meta])*] expires $name:ident { $($fields:tt)* }) => {
        $crate::wire::macros::messages!(@command_struct [
            $(#[$meta])*
            #[doc = ""]
            #[doc = "Expires: refused as `TooLate` when older than `COMMAND_EXPIRY_US` (protocol.md §5.3)."]
        ] $name { $($fields)* });
    };
    (@command_struct [$(#[$meta:meta])*] $name:ident { $($fields:tt)* }) => {
        $crate::wire::macros::wire_struct!($(#[$meta])* $name {
            request_id: RequestId,
            /// Client's session clock in µs when the pilot gave the command; a resend keeps it.
            sent_at: u32,
            $($fields)*
        });
    };
    (@expires expires) => { true };
    (@expires) => { false };
    // Only compiles if an expiring command can be refused as too late. `_message`: unused for commands without fields
    // or expiry.
    (@too_late expires $name:ident $message:ident) => {
        Some(Reply::to(&$message, Outcome::Refused(<$name as Request>::Refusal::TooLate)))
    };
    (@too_late $name:ident $message:ident) => { None };
    // Builds `Command` one variant at a time, so a command without fields is a unit variant (`Command::Arm`).
    (@command_enum [$($variants:tt)*]) => {
        /// What the pilot asks the drone to do, whichever protocol it came over (protocol.md §3): a command message
        /// without its Request ID and Sent At, which only our protocol has. Also the internal state's type, as
        /// `domain::Command`.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Command {
            $($variants)*
        }
    };
    (@command_enum [$($variants:tt)*] [$(#[$meta:meta])*] $name:ident {} $($rest:tt)*) => {
        $crate::wire::macros::messages!(@command_enum [$($variants)* $(#[$meta])* $name,] $($rest)*);
    };
    (@command_enum [$($variants:tt)*] [$(#[$meta:meta])*] $name:ident { $($fields:tt)+ } $($rest:tt)*) => {
        $crate::wire::macros::messages!(@command_enum [$($variants)* $(#[$meta])* $name { $($fields)+ },] $($rest)*);
    };
    (
        others { $($(#[$meta:meta])* $name:ident = $type:literal $({ $($fields:tt)* })?,)* }
        commands {
            $(
                $(#[$request_meta:meta])* $request:ident = $request_type:literal $($expires:ident)? {
                    $($(#[$field_meta:meta])* $field:ident: $field_type:ty),* $(,)?
                }
                $(refused $refusal:ident { $($reason:ident),* $(,)? })?,
            )*
        }
    ) => {
        $($crate::wire::macros::messages!(@struct $(#[$meta])* $name $({ $($fields)* })?);)*

        $(
            $crate::wire::macros::messages!(
                @command_struct [$(#[$request_meta])*] $($expires)? $request {
                    $($(#[$field_meta])* $field: $field_type,)*
                }
            );

            $(
                #[doc = concat!("The `Reason`s ", stringify!($request), " may be refused for, with their numbers.")]
                #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
                #[repr(u16)]
                pub enum $refusal {
                    $($reason = Reason::$reason as u16,)*
                }

                impl From<$refusal> for Reason {
                    fn from(refusal: $refusal) -> Reason {
                        return match refusal {
                            $($refusal::$reason => Reason::$reason,)*
                        };
                    }
                }

                impl Code for $refusal {
                    type Repr = u16;

                    fn decode(value: u16) -> Result<Self, MessageError> {
                        return match Reason::decode(value)? {
                            $(Reason::$reason => Ok($refusal::$reason),)*
                            _ => Err(MessageError::InvalidValue), // a Reason this request is never refused for
                        };
                    }

                    fn encode(self) -> u16 {
                        return self as u16;
                    }
                }
            )?

            impl sealed::Sealed for $request {
                fn answer(outcome: Outcome<<Self as Request>::Refusal>) -> Answer {
                    return Answer::$request(outcome);
                }
            }

            impl Request for $request {
                type Refusal = $crate::wire::macros::messages!(@refusal $($refusal)?);

                fn request_id(&self) -> RequestId {
                    return self.request_id;
                }
            }
        )*

        #[allow(clippy::large_enum_variant)] // owned messages are as big as the biggest one (protocol.md §7.1)
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Message {
            $($name($name),)*
            Command(CommandMessage),
        }

        /// A command as our protocol sends it: the message of one of the commands. `Message::Command` holds one.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum CommandMessage {
            $($request($request),)*
        }

        $crate::wire::macros::messages!(
            @command_enum [] $([$(#[$request_meta])*] $request { $($(#[$field_meta])* $field: $field_type),* })*
        );

        impl Command {
            /// Whether the drone refuses it as too late when older than `COMMAND_EXPIRY_US` (protocol.md §5.3).
            pub fn expires(self) -> bool {
                return match self {
                    $(Command::$request { .. } => $crate::wire::macros::messages!(@expires $($expires)?),)*
                };
            }
        }

        impl CommandMessage {
            /// The message for `command`. `sent_at` is the client's session clock in µs when the pilot gave it.
            pub fn new(request_id: RequestId, sent_at: u32, command: Command) -> Self {
                let message = match command {
                    $(Command::$request { $($field),* } => {
                        CommandMessage::$request($request { request_id, sent_at, $($field),* })
                    })*
                };
                debug_assert_eq!(message.command(), command, "new and command don't agree");
                return message;
            }

            pub fn request_id(self) -> RequestId {
                return match self {
                    $(CommandMessage::$request(message) => message.request_id,)*
                };
            }

            pub fn sent_at(self) -> u32 {
                return match self {
                    $(CommandMessage::$request(message) => message.sent_at,)*
                };
            }

            /// What the pilot asked, without what only our protocol has.
            pub fn command(self) -> Command {
                return match self {
                    $(CommandMessage::$request(_message) => Command::$request { $($field: _message.$field),* },)*
                };
            }

            /// The Reply with an outcome that fits every command. A refusal is typed per command: match the
            /// command's message and use `Reply::to`.
            pub fn reply(self, outcome: Outcome<::core::convert::Infallible>) -> Reply {
                let reply = match self {
                    $(CommandMessage::$request(message) => Reply::to(&message, outcome.widen()),)*
                };
                debug_assert_eq!(reply.request_id(), self.request_id());
                return reply;
            }

            /// The Reply refusing it as too late (protocol.md §5.3); `None` for a command that never expires.
            pub fn refuse_too_late(self) -> Option<Reply> {
                let reply = match self {
                    $(CommandMessage::$request(_message) => {
                        $crate::wire::macros::messages!(@too_late $($expires)? $request _message)
                    })*
                };
                debug_assert_eq!(reply.is_some(), self.command().expires(), "refuse_too_late and expires don't agree");
                return reply;
            }

            fn message_type(self) -> u8 {
                return match self {
                    $(CommandMessage::$request(_) => $request_type,)*
                };
            }

            fn write_payload(&self, w: &mut Writer) {
                match self {
                    $(CommandMessage::$request(m) => m.write(w),)*
                }
            }
        }

        /// Which request a Reply answers, for the answers that don't depend on it ("malformed").
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum RequestType {
            $($request,)*
        }

        /// What a Reply says, typed per request (protocol.md §5.4), so a Reply can't combine Answers Type, Status
        /// and Reason in a way the protocol doesn't allow.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Answer {
            $($request(Outcome<<$request as Request>::Refusal>),)*
            Malformed(RequestType),   // the request didn't decode; never a Reason
            UnknownType(UnknownType), // always Status "unknown type", never a Reason
        }

        /// What a message type byte stands for.
        pub(crate) enum TypeKind {
            Request(RequestType),
            Other, // a known message that isn't a request
            Unknown(UnknownType),
        }

        impl TypeKind {
            pub(crate) fn of(value: u8) -> Self {
                return match value {
                    $($request_type => TypeKind::Request(RequestType::$request),)*
                    $($type)|* => TypeKind::Other,
                    _ => TypeKind::Unknown(UnknownType(value)),
                };
            }
        }

        impl Message {
            fn message_type(&self) -> u8 {
                return match self {
                    $(Message::$name(_) => $type,)*
                    Message::Command(command) => command.message_type(),
                };
            }

            fn read_payload(message_type: u8, r: &mut Reader) -> Result<Self, MessageError> {
                return match message_type {
                    $($type => Ok(Message::$name(Wire::read(r)?)),)*
                    $($request_type => Ok(Message::Command(CommandMessage::$request(Wire::read(r)?))),)*
                    _ => Err(MessageError::UnknownType(UnknownType(message_type))),
                };
            }

            fn write_payload(&self, w: &mut Writer) {
                match self {
                    $(Message::$name(m) => m.write(w),)*
                    Message::Command(command) => command.write_payload(w),
                }
            }
        }

        impl RequestType {
            fn type_byte(self) -> u8 {
                return match self {
                    $(RequestType::$request => $request_type,)*
                };
            }

            /// This request's own Outcome.
            fn outcome(self, status: Status, reason: u16) -> Result<Answer, MessageError> {
                return match self {
                    $(RequestType::$request => Ok(Answer::$request(Outcome::from_wire(status, reason)?)),)*
                };
            }
        }

        /// Answers Type, Status and Reason.
        impl Wire for Answer {
            fn read(r: &mut Reader) -> Result<Self, MessageError> {
                return Answer::from_wire(u8::read(r)?, Status::read(r)?, u16::read(r)?);
            }

            fn write(&self, w: &mut Writer) {
                match *self {
                    $(Answer::$request(outcome) => {
                        u8::write(&$request_type, w);
                        outcome.write(w);
                    })*
                    Answer::Malformed(request) => {
                        request.type_byte().write(w);
                        Status::Malformed.write(w);
                        0u16.write(w);
                    }
                    Answer::UnknownType(unknown) => {
                        unknown.0.write(w);
                        Status::UnknownType.write(w);
                        0u16.write(w);
                    }
                }
            }
        }
    };
}
pub(crate) use messages;
