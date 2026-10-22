//! Shared code for the ground client and the flight controller.
//!
//! `no_std` (no operating system or heap needed), so the same code runs on the
//! laptop, on the Pi, and on a microcontroller if the project ever moves to one.
#![no_std]

#[cfg(feature = "std")]
extern crate std;

pub mod domain;
pub mod protocol;

pub fn hello() -> &'static str {
    "hello from drone-common"
}
