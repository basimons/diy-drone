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
/// u32 session clock of the protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LocalTime(pub u64);

/// When an adapter last heard from the client, the same for every protocol (protocol.md §3). The flight code
/// decides from it when the link counts as lost (§6 rules 4 and 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct LinkHealth {
    heard: Option<Heard>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Heard {
    valid: LocalTime,   // the last valid message
    changed: LocalTime, // the last message showing the client is alive; never after `valid`
}

impl LinkHealth {
    /// A valid message that may be a repeat. Over CRSF the TX module repeats the last values when the client
    /// freezes, so only a changed counter channel is `heard_change` (§6 rule 5). The first message is a change.
    pub fn heard(self, now: LocalTime) -> Self {
        let changed = self.heard.map_or(now, |heard| return heard.changed);
        return self.with(now, changed);
    }

    /// A valid message that shows the client is alive: over our protocol every message, over CRSF a changed counter.
    pub fn heard_change(self, now: LocalTime) -> Self {
        return self.with(now, now);
    }

    pub fn last_valid(&self) -> Option<LocalTime> {
        return self.heard.map(|heard| return heard.valid);
    }

    pub fn last_change(&self) -> Option<LocalTime> {
        return self.heard.map(|heard| return heard.changed);
    }

    fn with(self, valid: LocalTime, changed: LocalTime) -> Self {
        debug_assert!(
            self.last_valid().is_none_or(|last| return last <= valid),
            "local time goes back: {:?} after {valid:?}",
            self.last_valid()
        );
        debug_assert!(changed <= valid, "changed after the last valid message");
        return Self {
            heard: Some(Heard { valid, changed }),
        };
    }
}

/// Stick positions (protocol.md §5.2). Defined with the messages, since the Sticks message is exactly this.
pub use crate::wire::message::Sticks;
