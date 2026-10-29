use std::path::PathBuf;
use std::time::{Duration, Instant};

use drone_voice::{BackgroundCell, PromptCell, QueuedTake};

use crate::{Button, Effect, Input};

const REMOVE_HOLD: Duration = Duration::from_secs(1);

/// Walking through the queued prompts of one session.
#[derive(Debug)]
pub struct TakeSession {
    queue: Vec<QueuedTake>,
    next: usize,                         // index of the current prompt; == queue.len() when finished
    last: Option<(PathBuf, QueuedTake)>, // the take ○ and △ act on
    kept: Vec<String>,                   // condition of every take kept this session
    phase: Phase,
    paused: bool,
    square: Square,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Ready,
    Recording,
    Saving, // R1 released; the take arrives after its post-roll, via `saved` or `not_saved`
}

#[derive(Debug, PartialEq, Eq)]
enum Square {
    Up,
    Down(Instant),
    Removed, // held long enough to remove the cell; waits for the release
}

impl TakeSession {
    pub fn new(queue: Vec<QueuedTake>) -> TakeSession {
        return TakeSession {
            queue,
            next: 0,
            last: None,
            kept: Vec::new(),
            phase: Phase::Ready,
            paused: false,
            square: Square::Up,
        };
    }

    /// `None` once every queued prompt is done; ○ and △ still work then, ✕ finishes.
    pub fn current(&self) -> Option<&QueuedTake> {
        return self.queue.get(self.next);
    }

    /// (take number, takes in this session), counting from 1.
    pub fn position(&self) -> (usize, usize) {
        return (self.next + 1, self.queue.len());
    }

    pub fn is_paused(&self) -> bool {
        return self.paused;
    }

    pub fn is_recording(&self) -> bool {
        return self.phase == Phase::Recording;
    }

    /// The take being recorded or saved was lost (mic gone, disk error); the current prompt stays.
    pub fn not_saved(&mut self) {
        self.phase = Phase::Ready;
    }

    pub fn handle(&mut self, input: Input, now: Instant) -> Vec<Effect> {
        if self.paused {
            return match input {
                Input::Down(Button::Options) => {
                    self.paused = false;
                    vec![]
                }
                Input::Down(Button::Cross) => vec![Effect::End],
                _ => vec![],
            };
        }
        return match (self.phase, input) {
            (Phase::Recording, Input::Up(Button::R1)) => {
                self.phase = Phase::Saving;
                vec![Effect::StopTake]
            }
            // The previous take is saved first (it completes on the press), so there must be a prompt after it.
            (Phase::Saving, Input::Down(Button::R1)) if self.next + 1 < self.queue.len() => self.start_take(),
            (Phase::Recording | Phase::Saving, _) => vec![],
            (Phase::Ready, input) => self.handle_ready(input, now),
        };
    }

    fn handle_ready(&mut self, input: Input, now: Instant) -> Vec<Effect> {
        return match input {
            Input::Down(Button::R1) if self.current().is_some() => self.start_take(),
            Input::Down(Button::Cross) if self.current().is_none() => vec![Effect::End],
            Input::Down(Button::Options) => {
                self.paused = true;
                self.square = Square::Up;
                vec![]
            }
            Input::Down(Button::Circle) => self.redo(),
            Input::Down(Button::Triangle) => self
                .last
                .iter()
                .map(|(file, _)| return Effect::Play(file.clone()))
                .collect(),
            Input::Down(Button::Square) => {
                self.square = Square::Down(now);
                vec![]
            }
            Input::Up(Button::Square) => {
                if matches!(self.square, Square::Down(_)) {
                    self.drop_current_cell();
                }
                self.square = Square::Up;
                vec![]
            }
            _ => vec![],
        };
    }

    fn start_take(&mut self) -> Vec<Effect> {
        self.phase = Phase::Recording;
        self.square = Square::Up;
        return vec![Effect::StartTake];
    }

    /// Call every frame: holding □ for a second removes the current cell.
    pub fn tick(&mut self, now: Instant) -> Vec<Effect> {
        let Square::Down(since) = self.square else {
            return vec![];
        };
        if now.duration_since(since) < REMOVE_HOLD {
            return vec![];
        }
        self.square = Square::Removed;
        return self.drop_current_cell().into_iter().map(Effect::Remove).collect();
    }

    /// The take for the current prompt is on disk; move on.
    pub fn saved(&mut self, file: PathBuf) {
        debug_assert!(self.current().is_some(), "a take was saved after the session finished");
        let Some(take) = self.current().cloned() else {
            return;
        };
        self.kept.push(take.cell.condition_id.clone());
        self.last = Some((file, take));
        self.next += 1;
        if self.phase == Phase::Saving {
            self.phase = Phase::Ready;
        }
    }

    /// Takes kept this session per condition, in the order the conditions first came up.
    pub fn summary(&self) -> Vec<(String, u32)> {
        return self
            .kept
            .iter()
            .fold(Vec::new(), |mut counts: Vec<(String, u32)>, condition| {
                match counts.iter_mut().find(|(c, _)| return c == condition) {
                    Some((_, count)) => *count += 1,
                    None => counts.push((condition.clone(), 1)),
                }
                return counts;
            });
    }

    fn redo(&mut self) -> Vec<Effect> {
        let Some((file, take)) = self.last.take() else {
            return vec![];
        };
        if let Some(i) = self.kept.iter().rposition(|c| return *c == take.cell.condition_id) {
            self.kept.remove(i);
        }
        self.next -= 1;
        debug_assert_eq!(self.queue[self.next], take);
        return vec![Effect::Reject(file)];
    }

    /// Drops every remaining queued take of the current cell; returns that cell.
    fn drop_current_cell(&mut self) -> Option<PromptCell> {
        let cell = self.current()?.cell.clone();
        let (done, rest) = self.queue.split_at(self.next);
        self.queue = done
            .iter()
            .chain(rest.iter().filter(|take| return take.cell != cell))
            .cloned()
            .collect();
        return Some(cell);
    }
}

/// Hands-free background recording in one condition.
#[derive(Debug)]
pub struct BackgroundSession {
    cell: BackgroundCell,
    recording: bool,
    chunks: Vec<(PathBuf, Duration)>,
}

impl BackgroundSession {
    pub fn new(cell: BackgroundCell) -> BackgroundSession {
        return BackgroundSession {
            cell,
            recording: false,
            chunks: Vec::new(),
        };
    }

    pub fn cell(&self) -> &BackgroundCell {
        return &self.cell;
    }

    pub fn handle(&mut self, input: Input) -> Vec<Effect> {
        return match (input, self.recording) {
            (Input::Down(Button::R1), false) => {
                self.recording = true;
                vec![Effect::StartBackground]
            }
            (Input::Down(Button::R1), true) => {
                self.recording = false;
                vec![Effect::StopBackground]
            }
            (Input::Down(Button::Circle), false) => self
                .chunks
                .pop()
                .map(|(file, _)| return Effect::Reject(file))
                .into_iter()
                .collect(),
            (Input::Down(Button::Triangle), false) => self
                .chunks
                .last()
                .map(|(file, _)| return Effect::Play(file.clone()))
                .into_iter()
                .collect(),
            (Input::Down(Button::Options), false) => vec![Effect::End],
            _ => vec![],
        };
    }

    pub fn chunk_saved(&mut self, file: PathBuf, length: Duration) {
        self.chunks.push((file, length));
    }

    /// The chunker reached the condition's target.
    pub fn stopped(&mut self) {
        self.recording = false;
    }

    pub fn is_recording(&self) -> bool {
        return self.recording;
    }

    /// Kept this session.
    pub fn recorded(&self) -> Duration {
        return self.chunks.iter().map(|(_, length)| return *length).sum();
    }
}
