//! The recorder's screens as state machines: button presses in, effects out. No I/O and no clocks.

mod session;
mod setup;

use std::path::PathBuf;

use drone_voice::PromptCell;

pub use session::{BackgroundSession, TakeSession};
pub use setup::{Row, Setup, Start};

/// The controller buttons the recorder uses (PlayStation names; the keyboard mirrors them).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    R1,
    Cross,
    Circle,
    Triangle,
    Square,
    Options,
    Up,
    Down,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Down(Button),
    Up(Button),
}

/// What the GUI shell has to do in response to an input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    StartTake,
    StopTake,
    StartBackground,
    StopBackground,
    Reject(PathBuf),
    Play(PathBuf),
    Remove(PromptCell),
    End,
}
