//! Voice commands on the ground side. Design: docs/superpowers/specs/2026-10-29-voice-recorder-design.md.

mod plan;

pub use plan::{BackgroundCell, Condition, Plan, PlanError, PromptCell, PromptKind, slug};
