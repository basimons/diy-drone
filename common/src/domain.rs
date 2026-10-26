pub const CHANNEL_COUNT: usize = 16;

/// CRSF channel values are 11-bit; these are the usual stick limits.
pub const CHANNEL_MIN: u16 = 172;
pub const CHANNEL_MID: u16 = 992;
pub const CHANNEL_MAX: u16 = 1811;

/// In raw CRSF units.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RcChannels(pub [u16; CHANNEL_COUNT]);

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

/// Stick positions, protocol-neutral (protocol.md §5.2). What they mean is up to the flight code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Sticks {
    pub throttle: u16, // 0 … 65535, no centre
    pub roll: i16,     // 0 = centred
    pub pitch: i16,
    pub yaw: i16,
}

/// Something the pilot asks the drone to do (protocol.md §5.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Command {
    Arm,
    Disarm,
    TakeOff,
    Land,
    GoHome, // not supported until GPS is fitted
    Flip(Flip),
    SteadyState, // hold still and accept only a few requests
    ExitSteadyState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Flip {
    pub direction: u16, // 0 = forward, clockwise seen from above, a full circle over 0 … 65535
    pub duration_ms: u16,
}
