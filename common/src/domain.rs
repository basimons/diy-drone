//! Domain objects: what the drone and the client talk about.

/// Number of RC channels carried in one CRSF RC frame.
pub const CHANNEL_COUNT: usize = 16;

/// CRSF channel values are 11-bit numbers. These are the usual stick limits.
pub const CHANNEL_MIN: u16 = 172;
pub const CHANNEL_MID: u16 = 992;
pub const CHANNEL_MAX: u16 = 1811;

/// The stick/switch positions sent from the transmitter, in raw CRSF units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RcChannels(pub [u16; CHANNEL_COUNT]);

impl Default for RcChannels {
    fn default() -> Self {
        return Self([CHANNEL_MID; CHANNEL_COUNT]);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ArmState {
    #[default]
    Disarmed,
    Armed,
}
