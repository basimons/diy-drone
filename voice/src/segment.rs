//! Cutting the continuous mic stream into takes: push-to-talk segments and fixed-length background chunks.
//! Time is counted in samples, never with a clock.

use std::collections::VecDeque;
use std::time::Duration;

use crate::plan::PromptKind;
use crate::store::Warning;

const PRE_ROLL: Duration = Duration::from_millis(300);
const POST_ROLL: Duration = Duration::from_millis(300);
const CHUNK: Duration = Duration::from_secs(60);
const CLIPPED_PEAK: u32 = 32_000;
const QUIET_PEAK: u32 = 1_000;
const SHORTEST_HOLD: Duration = Duration::from_millis(300);
const LONGEST_HOLD: Duration = Duration::from_secs(4);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub samples: Vec<i16>,
    pub held: Duration, // audio time between press and release
}

#[derive(Debug)]
enum State {
    Idle,
    Held {
        samples: Vec<i16>,
        held: usize,
    },
    PostRoll {
        samples: Vec<i16>,
        held: usize,
        left: usize,
    },
}

/// Turns R1 presses and releases plus the mic stream into takes that start 300 ms before the press and end 300 ms
/// after the release.
#[derive(Debug)]
pub struct PushToTalk {
    sample_rate: u32,
    recent: VecDeque<i16>, // the last PRE_ROLL of audio, whatever the state
    state: State,
}

fn samples_in(duration: Duration, sample_rate: u32) -> usize {
    return (duration.as_millis() as usize) * sample_rate as usize / 1000;
}

impl PushToTalk {
    pub fn new(sample_rate: u32) -> PushToTalk {
        debug_assert!(sample_rate > 0);
        return PushToTalk {
            sample_rate,
            recent: VecDeque::new(),
            state: State::Idle,
        };
    }

    /// Starts a take. A take still in its post-roll is finished early and returned.
    pub fn press(&mut self) -> Option<Segment> {
        let start = State::Held {
            samples: self.recent.iter().copied().collect(),
            held: 0,
        };
        return match std::mem::replace(&mut self.state, State::Idle) {
            State::Idle => {
                self.state = start;
                None
            }
            held @ State::Held { .. } => {
                self.state = held;
                None
            }
            State::PostRoll { samples, held, .. } => {
                self.state = start;
                Some(self.segment(samples, held))
            }
        };
    }

    /// Ends the take after the post-roll. Ignored when nothing is held (R1 may be down when the app starts).
    pub fn release(&mut self) {
        if let State::Held { samples, held } = std::mem::replace(&mut self.state, State::Idle) {
            self.state = State::PostRoll {
                samples,
                held,
                left: samples_in(POST_ROLL, self.sample_rate),
            };
        }
    }

    /// Takes the next block of audio; returns a take once its post-roll is complete.
    pub fn feed(&mut self, block: &[i16]) -> Option<Segment> {
        let pre_roll = samples_in(PRE_ROLL, self.sample_rate);
        self.recent.extend(block);
        let excess = self.recent.len().saturating_sub(pre_roll);
        self.recent.drain(..excess);

        return match &mut self.state {
            State::Idle => None,
            State::Held { samples, held } => {
                samples.extend_from_slice(block);
                *held += block.len();
                None
            }
            State::PostRoll { samples, left, .. } => {
                let taken = block.len().min(*left);
                samples.extend_from_slice(&block[..taken]);
                *left -= taken;
                if *left > 0 {
                    return None;
                }
                let State::PostRoll { samples, held, .. } = std::mem::replace(&mut self.state, State::Idle) else {
                    unreachable!("matched PostRoll above");
                };
                Some(self.segment(samples, held))
            }
        };
    }

    fn segment(&self, samples: Vec<i16>, held: usize) -> Segment {
        return Segment {
            samples,
            held: Duration::from_millis(held as u64 * 1000 / u64::from(self.sample_rate)),
        };
    }
}

/// Cuts a hands-free background recording into 60 s chunks and stops taking audio at `limit`.
#[derive(Debug)]
pub struct Chunker {
    chunk: usize,
    left: usize, // samples still to take before the limit
    current: Vec<i16>,
}

impl Chunker {
    pub fn new(sample_rate: u32, limit: Duration) -> Chunker {
        return Chunker {
            chunk: samples_in(CHUNK, sample_rate),
            left: samples_in(limit, sample_rate),
            current: Vec::new(),
        };
    }

    /// The chunks completed by this block.
    pub fn feed(&mut self, block: &[i16]) -> Vec<Vec<i16>> {
        let taken = &block[..block.len().min(self.left)];
        self.left -= taken.len();
        let mut done = Vec::new();
        for &sample in taken {
            self.current.push(sample);
            if self.current.len() == self.chunk {
                done.push(std::mem::take(&mut self.current));
            }
        }
        return done;
    }

    pub fn is_done(&self) -> bool {
        return self.left == 0;
    }

    /// The last, partial chunk, if any audio is in it.
    pub fn finish(self) -> Option<Vec<i16>> {
        return if self.current.is_empty() {
            None
        } else {
            Some(self.current)
        };
    }
}

/// Things worth a second look before keeping a take. None of them stop the take from being saved.
pub fn warnings(kind: PromptKind, segment: &Segment) -> Vec<Warning> {
    let peak = segment
        .samples
        .iter()
        .map(|s| return u32::from(s.unsigned_abs()))
        .max()
        .unwrap_or(0);
    let short_prompt = matches!(kind, PromptKind::Command | PromptKind::WakeWord | PromptKind::Phrase);
    return [
        (peak >= CLIPPED_PEAK, Warning::Clipped),
        (peak < QUIET_PEAK, Warning::VeryQuiet),
        (segment.held < SHORTEST_HOLD, Warning::TooShort),
        (short_prompt && segment.held > LONGEST_HOLD, Warning::TooLong),
    ]
    .into_iter()
    .filter(|(raised, _)| return *raised)
    .map(|(_, warning)| return warning)
    .collect();
}
