//! The session clock (protocol.md §5.1).

use core::time::Duration;

/// Half the range of the session clock (~35 minutes): two times in unknown order are compared correctly as long as they
/// are less than this apart. Every span the crate measures stays below it.
pub const MAX_SPAN: Duration = Duration::from_micros(1 << 31);

/// A moment on this side's session clock: µs since the session started (protocol.md §5.1). Wraps after ~71 minutes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SessionTime(pub u32);

impl SessionTime {
    /// The time from `earlier` to `self`, with wrapping arithmetic: right whenever `earlier` really is earlier, by less
    /// than ~71 minutes. Two times in unknown order can only be compared within `MAX_SPAN` (protocol.md §5.1).
    pub fn since(self, earlier: SessionTime) -> Duration {
        return Duration::from_micros(u64::from(self.0.wrapping_sub(earlier.0)));
    }
}
