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

/// The transmitter's arm switch. Arm is only accepted while it is on (drone-design.md, "Failsafes").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ArmSwitch {
    #[default]
    Off,
    On,
}

/// A point in time on this side's own clock, in µs since it started. A u64 never wraps (585 000 years), unlike the
/// protocol's u32 session clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uptime(pub u64);

/// When an adapter last heard from the client, the same for every protocol (protocol.md §3). The flight code
/// decides from it when the link counts as lost (§6 rules 4 and 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct LinkHealth {
    last_heard: Option<Heard>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Heard {
    pub valid: Uptime, // the last valid message
    pub fresh: Uptime, // the last message showing the client is alive; never after `valid`
}

impl LinkHealth {
    /// A valid message that shows the client is alive: over our protocol every message, over CRSF a changed
    /// counter channel.
    pub fn heard_fresh(self, now: Uptime) -> Self {
        return self.with(now, now);
    }

    /// A valid message that may repeat old values: over CRSF the TX module repeats the last frame when the client
    /// freezes (§6 rule 5). The first message counts as fresh.
    pub fn heard_repeat(self, now: Uptime) -> Self {
        let fresh = self.last_heard.map_or(now, |heard| return heard.fresh);
        return self.with(now, fresh);
    }

    /// `None` until the first valid message.
    pub fn last_heard(&self) -> Option<Heard> {
        return self.last_heard;
    }

    fn with(self, valid: Uptime, fresh: Uptime) -> Self {
        debug_assert!(
            self.last_heard.is_none_or(|last| return last.valid <= valid),
            "uptime goes back: {valid:?} after {:?}",
            self.last_heard
        );
        let fresh = fresh.min(valid); // only differs if the clock went back, which the assert above reports
        return Self {
            last_heard: Some(Heard { valid, fresh }),
        };
    }
}

/// Stick positions (protocol.md §5.2). Defined with the messages, since the Sticks message is exactly this.
pub use crate::wire::message::Sticks;
