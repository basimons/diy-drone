//! Voice commands on the ground side. Design: docs/superpowers/specs/2026-10-29-voice-recorder-design.md.

mod mic;
mod picker;
mod plan;
mod segment;
mod store;
mod wav;

pub use mic::{Mic, MicError, MicEvent, capture_volume};
pub use picker::{QueuedTake, Selection, pick};
pub use plan::{BackgroundCell, Condition, Plan, PlanError, PromptCell, PromptKind, slug};
pub use segment::{Chunker, PushToTalk, Segment, warnings};
pub use store::{Clip, MicInfo, NewTake, Progress, Store, StoreError, Warning};
pub use wav::read_wav;
