//! Shared by client and flight controller. `no_std` so it also runs without an OS or heap.
#![no_std]

// Tests may use std (Vec); the crate itself stays no_std.
#[cfg(any(feature = "std", test))]
extern crate std;

pub mod domain;
pub mod protocol;
pub mod wire;

pub fn hello() -> &'static str {
    return "hello from drone-common";
}
