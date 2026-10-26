/// Defines every message from one list: its struct (unless it is defined elsewhere, like `Reply`), its type byte,
/// `Message` over all of them, and for each request its refusal codes, its `Answer` variant and its `RequestType`.
/// A request's struct starts with its Request ID. `refused` lists the `Reason`s it may be refused for, as its own
/// enum; one without `refused` is never refused.
macro_rules! messages {
    (@refusal) => { ::core::convert::Infallible };
    (@refusal $refusal:ident) => { $refusal };
    (@struct $(#[$meta:meta])* $name:ident) => {}; // defined elsewhere
    (@struct $(#[$meta:meta])* $name:ident { $($fields:tt)* }) => {
        $crate::wire::macros::wire_struct!($(#[$meta])* $name { $($fields)* });
    };
    (
        others { $($(#[$meta:meta])* $name:ident = $type:literal $({ $($fields:tt)* })?,)* }
        requests {
            $(
                $(#[$request_meta:meta])* $request:ident = $request_type:literal { $($request_fields:tt)* }
                $(refused $refusal:ident { $($reason:ident),* $(,)? })?,
            )*
        }
    ) => {
        $($crate::wire::macros::messages!(@struct $(#[$meta])* $name $({ $($fields)* })?);)*

        $(
            $crate::wire::macros::wire_struct!($(#[$request_meta])* $request {
                request_id: RequestId,
                $($request_fields)*
            });

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

                    fn from_repr(value: u16) -> Result<Self, MessageError> {
                        return match Reason::from_repr(value)? {
                            $(Reason::$reason => Ok($refusal::$reason),)*
                            _ => Err(MessageError::InvalidValue), // a Reason this request is never refused for
                        };
                    }

                    fn repr(self) -> u16 {
                        return self as u16;
                    }
                }
            )?

            impl sealed::Sealed for $request {}

            impl Request for $request {
                type Refusal = $crate::wire::macros::messages!(@refusal $($refusal)?);

                fn request_id(&self) -> RequestId {
                    return self.request_id;
                }

                fn answer(outcome: Outcome<Self::Refusal>) -> Answer {
                    return Answer::$request(outcome);
                }
            }
        )*

        #[allow(clippy::large_enum_variant)] // owned messages are as big as the biggest one (protocol.md §7.1)
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Message {
            $($name($name),)*
            $($request($request),)*
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
                    $(Message::$request(_) => $request_type,)*
                };
            }

            fn read_payload(message_type: u8, r: &mut Reader) -> Result<Self, MessageError> {
                return match message_type {
                    $($type => Ok(Message::$name(Wire::read(r)?)),)*
                    $($request_type => Ok(Message::$request(Wire::read(r)?)),)*
                    _ => Err(MessageError::UnknownType(UnknownType(message_type))),
                };
            }

            fn write_payload(&self, w: &mut Writer) {
                match self {
                    $(Message::$name(m) => m.write(w),)*
                    $(Message::$request(m) => m.write(w),)*
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
