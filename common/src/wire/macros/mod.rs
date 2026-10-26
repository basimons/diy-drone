//! The four macros that turn the protocol's lists into types: each message, event and code is written once.

mod code;
mod events;
mod messages;
mod wire_struct;

pub(crate) use code::code;
pub(crate) use events::events;
pub(crate) use messages::messages;
pub(crate) use wire_struct::wire_struct;
