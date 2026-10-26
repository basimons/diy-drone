pub const CHANNEL_COUNT: usize = 16;

/// CRSF channel values are 11-bit; these are the usual stick limits.
pub const CHANNEL_MIN: u16 = 172;
pub const CHANNEL_MID: u16 = 992;
pub const CHANNEL_MAX: u16 = 1811;

/// The largest 11-bit value.
pub const CHANNEL_LIMIT: u16 = 0x07FF;

/// In raw CRSF units, each 0 … `CHANNEL_LIMIT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RcChannels([u16; CHANNEL_COUNT]);

impl RcChannels {
    /// `None` if a value doesn't fit in 11 bits.
    pub fn new(values: [u16; CHANNEL_COUNT]) -> Option<Self> {
        return if values.iter().all(|&v| return v <= CHANNEL_LIMIT) {
            Some(Self(values))
        } else {
            None
        };
    }

    pub fn values(&self) -> [u16; CHANNEL_COUNT] {
        return self.0;
    }
}

impl Default for RcChannels {
    fn default() -> Self {
        return Self([CHANNEL_MID; CHANNEL_COUNT]);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ArmState {
    #[default]
    Disarmed,
    Armed,
}

/// Stick positions (protocol.md §5.2). Defined with the messages, since the Sticks message is exactly this.
pub use crate::wire::message::Sticks;
