//! Shared code for the ground client and the flight controller.
//!
//! `no_std` (no operating system or heap needed), so the same code runs on the
//! laptop, on the Pi, and on a microcontroller if the project ever moves to one.
#![no_std]

// Tests may use the standard library (Vec, formatting) even though the crate itself does not.
#[cfg(any(feature = "std", test))]
extern crate std;

pub mod domain;
pub mod protocol;
pub mod wire;

pub fn hello() -> &'static str {
    "hello from drone-common"
}
