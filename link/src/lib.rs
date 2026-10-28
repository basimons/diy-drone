//! Link behaviour both sides need (protocol.md §7): what to acknowledge, what is lost, the round-trip time, which IDs
//! were already handled. It says what is true about the link; what to do about it is up to the caller. No I/O and no
//! clocks: the caller passes in the time. `no_std` with no heap, like `drone-common`.
#![no_std]

pub mod ack;
pub mod outbox;
pub mod recent_ids;
pub mod rtt;
pub mod time;
