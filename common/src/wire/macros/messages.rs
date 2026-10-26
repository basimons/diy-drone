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

                    /// A known `Reason` this request can't have is invalid, not unknown.
                    fn from_repr(value: u16) -> Result<Self, MessageError> {
                        Reason::from_repr(value)?;
                        return [$($refusal::$reason),*]
                            .into_iter()
                            .find(|refusal| return *refusal as u16 == value)
                            .ok_or(MessageError::InvalidValue);
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

                fn answer(outcome: Result<(), Failure<Self::Refusal>>) -> Answer {
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
            $($request(Result<(), Failure<<$request as Request>::Refusal>>),)*
            Malformed(RequestType),   // the request didn't decode; never a Reason
            UnknownType(UnknownType), // always Status "unknown type", never a Reason
        }

        fn is_known_type(value: u8) -> bool {
            return matches!(value, $($type)|* | $($request_type)|*);
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
            fn from_type_byte(value: u8) -> Option<Self> {
                return match value {
                    $($request_type => Some(RequestType::$request),)*
                    _ => None,
                };
            }

            fn type_byte(self) -> u8 {
                return match self {
                    $(RequestType::$request => $request_type,)*
                };
            }

            /// The Answer of this request with this Status and Reason.
            fn answer(self, status: Status, reason: u16) -> Result<Answer, MessageError> {
                return match self {
                    $(RequestType::$request => Ok(Answer::$request(outcome_from_wire(status, reason)?)),)*
                };
            }
        }

        impl Answer {
            /// Answers Type, Status and Reason.
            pub(crate) fn to_wire(self) -> (u8, (Status, u16)) {
                return match self {
                    $(Answer::$request(outcome) => ($request_type, outcome_to_wire(outcome)),)*
                    Answer::Malformed(request) => (request.type_byte(), (Status::Malformed, 0)),
                    Answer::UnknownType(unknown) => (unknown.0, (Status::UnknownType, 0)),
                };
            }
        }
    };
}
pub(crate) use messages;
