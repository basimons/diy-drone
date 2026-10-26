//! Shared by client and flight controller. `no_std` so it also runs without an OS or heap.
#![no_std]

#[cfg(feature = "std")]
extern crate std;

pub mod crsf;
pub mod domain;
pub mod wire;
