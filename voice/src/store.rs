//! Takes on disk: WAV files, the append-only manifest, removed cells, and progress counted from the manifest.
//! Layout and columns: docs/superpowers/specs/2026-10-29-voice-recorder-design.md, "Files and data".

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::plan::{BackgroundCell, PromptCell, slug};
use crate::wav::write_wav;

const MANIFEST: &str = "manifest.csv";
const REMOVED: &str = "removed.csv";
const TIME_IN_CSV: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";
const TIME_IN_FILE_NAME: &str = "%Y-%m-%dT%H-%M-%S%.3fZ";
const BACKGROUND: &str = "background";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MicInfo {
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16, // what the device delivers; stored WAVs are always mono
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Warning {
    Clipped,
    VeryQuiet,
    TooShort,
    TooLong,
}

impl Warning {
    pub fn as_str(self) -> &'static str {
        return match self {
            Warning::Clipped => "clipped",
            Warning::VeryQuiet => "very_quiet",
            Warning::TooShort => "too_short",
            Warning::TooLong => "too_long",
        };
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Clip<'a> {
    Prompt(&'a PromptCell),
    Background(&'a BackgroundCell),
}

pub struct NewTake<'a> {
    pub clip: Clip<'a>,
    pub samples: &'a [i16],
    pub style: Option<&'a str>,
    pub speaker: &'a str,
    pub session_id: &'a str,
    pub time: DateTime<Utc>,
    pub mic: &'a MicInfo,
    pub capture_volume: Option<&'a str>,
    pub warnings: &'a [Warning],
}

#[derive(Debug)]
pub enum StoreError {
    Io(String),
    Csv(String),
    Wav(String),
    UnknownFile(PathBuf), // no take in the manifest has this file, or it was rejected already
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        return match self {
            StoreError::Io(e) => write!(f, "file error: {e}"),
            StoreError::Csv(e) => write!(f, "manifest error: {e}"),
            StoreError::Wav(e) => write!(f, "WAV error: {e}"),
            StoreError::UnknownFile(file) => write!(f, "no take is saved as {}", file.display()),
        };
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> StoreError {
        return StoreError::Io(e.to_string());
    }
}

impl From<csv::Error> for StoreError {
    fn from(e: csv::Error) -> StoreError {
        return StoreError::Csv(e.to_string());
    }
}

impl From<hound::Error> for StoreError {
    fn from(e: hound::Error) -> StoreError {
        return StoreError::Wav(e.to_string());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Event {
    Take,
    Rejected,
}

/// One manifest line. Field order is the column order.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Row {
    time_utc: String,
    session_id: String,
    event: Event,
    file: String,
    kind: String,
    prompt_id: String,
    prompt_text: String,
    condition_id: String,
    style: String,
    speaker: String,
    duration_ms: u64,
    sample_rate: u32,
    channels: u16,
    device_name: String,
    capture_volume: String,
    peak: u32,
    warnings: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Removal {
    prompt_id: String,
    condition_id: String,
    time_utc: String,
}

pub struct Store {
    root: PathBuf,
    rows: Vec<Row>,
    removed: HashSet<(String, String)>,
    manifest: csv::Writer<File>,
    removals: csv::Writer<File>,
}

/// How much is recorded per cell, counted from the manifest: kept takes, minus rejected ones.
#[derive(Debug, Default)]
pub struct Progress {
    takes: HashMap<(String, String), u32>,
    recorded: HashMap<String, Duration>,
}

impl Progress {
    pub fn takes(&self, cell: &PromptCell) -> u32 {
        return self
            .takes
            .get(&(cell.prompt_id.clone(), cell.condition_id.clone()))
            .copied()
            .unwrap_or(0);
    }

    pub fn recorded(&self, cell: &BackgroundCell) -> Duration {
        return self.recorded.get(&cell.condition_id).copied().unwrap_or_default();
    }
}

impl Store {
    pub fn open(root: &Path) -> Result<Store, StoreError> {
        fs::create_dir_all(root)?;
        let (rows, manifest) = open_csv::<Row>(&root.join(MANIFEST))?;
        let (removals, removals_writer) = open_csv::<Removal>(&root.join(REMOVED))?;
        let removed = removals
            .into_iter()
            .map(|r| return (r.prompt_id, r.condition_id))
            .collect();
        return Ok(Store {
            root: root.to_path_buf(),
            rows,
            removed,
            manifest,
            removals: removals_writer,
        });
    }

    pub fn root(&self) -> &Path {
        return &self.root;
    }

    /// Writes the WAV, then its manifest row; returns the WAV's path relative to the root.
    pub fn save(&mut self, take: NewTake<'_>) -> Result<PathBuf, StoreError> {
        debug_assert!(take.mic.sample_rate > 0);
        let (kind, dir, prompt_id, prompt_text, condition_id) = match take.clip {
            Clip::Prompt(cell) => (
                cell.kind.as_str(),
                cell.kind.dir(),
                &*cell.prompt_id,
                &*cell.text,
                &*cell.condition_id,
            ),
            Clip::Background(cell) => (BACKGROUND, BACKGROUND, BACKGROUND, "", &*cell.condition_id),
        };
        let name = format!(
            "{}__{}__{}.wav",
            slug(take.speaker),
            take.time.format(TIME_IN_FILE_NAME),
            take.style.map_or_else(|| return "none".to_string(), slug)
        );
        let file: PathBuf = ["takes", dir, prompt_id, condition_id, &name].iter().collect();
        let path = self.root.join(&file);
        fs::create_dir_all(path.parent().expect("a take path has folders"))?;
        write_wav(&path, take.samples, take.mic.sample_rate)?;

        let row = Row {
            time_utc: take.time.format(TIME_IN_CSV).to_string(),
            session_id: take.session_id.to_string(),
            event: Event::Take,
            file: file_column(&file),
            kind: kind.to_string(),
            prompt_id: prompt_id.to_string(),
            prompt_text: prompt_text.to_string(),
            condition_id: condition_id.to_string(),
            style: take.style.unwrap_or("").to_string(),
            speaker: take.speaker.to_string(),
            duration_ms: take.samples.len() as u64 * 1000 / u64::from(take.mic.sample_rate),
            sample_rate: take.mic.sample_rate,
            channels: take.mic.channels,
            device_name: take.mic.device_name.clone(),
            capture_volume: take.capture_volume.unwrap_or("").to_string(),
            peak: take
                .samples
                .iter()
                .map(|s| return s.unsigned_abs() as u32)
                .max()
                .unwrap_or(0),
            warnings: take
                .warnings
                .iter()
                .map(|w| return w.as_str())
                .collect::<Vec<_>>()
                .join(";"),
        };
        self.manifest.serialize(&row)?;
        self.manifest.flush()?;
        self.rows.push(row);
        return Ok(file);
    }

    /// Moves a take's WAV to `rejected/` and records that it no longer counts.
    pub fn reject(&mut self, file: &Path, time: DateTime<Utc>) -> Result<(), StoreError> {
        let column = file_column(file);
        let rejected: HashSet<&str> = self.rejected_files();
        let take = self
            .rows
            .iter()
            .find(|row| return row.event == Event::Take && row.file == column && !rejected.contains(row.file.as_str()))
            .ok_or_else(|| return StoreError::UnknownFile(file.to_path_buf()))?;
        let row = Row {
            time_utc: time.format(TIME_IN_CSV).to_string(),
            event: Event::Rejected,
            ..take.clone()
        };

        let aside = self.root.join("rejected").join(file);
        fs::create_dir_all(aside.parent().expect("a take path has folders"))?;
        fs::rename(self.root.join(file), aside)?;
        self.manifest.serialize(&row)?;
        self.manifest.flush()?;
        self.rows.push(row);
        return Ok(());
    }

    /// Never asks for this cell again (until its row in `removed.csv` is deleted by hand).
    pub fn remove(&mut self, cell: &PromptCell, time: DateTime<Utc>) -> Result<(), StoreError> {
        self.removals.serialize(Removal {
            prompt_id: cell.prompt_id.clone(),
            condition_id: cell.condition_id.clone(),
            time_utc: time.format(TIME_IN_CSV).to_string(),
        })?;
        self.removals.flush()?;
        self.removed.insert((cell.prompt_id.clone(), cell.condition_id.clone()));
        return Ok(());
    }

    pub fn is_removed(&self, cell: &PromptCell) -> bool {
        return self
            .removed
            .contains(&(cell.prompt_id.clone(), cell.condition_id.clone()));
    }

    pub fn last_speaker(&self) -> Option<&str> {
        return self
            .rows
            .iter()
            .rev()
            .find(|row| return row.event == Event::Take)
            .map(|row| return row.speaker.as_str());
    }

    pub fn progress(&self) -> Progress {
        let rejected = self.rejected_files();
        let kept = self
            .rows
            .iter()
            .filter(|row| return row.event == Event::Take && !rejected.contains(row.file.as_str()));
        return kept.fold(Progress::default(), |mut progress, row| {
            if row.kind == BACKGROUND {
                *progress.recorded.entry(row.condition_id.clone()).or_default() +=
                    Duration::from_millis(row.duration_ms);
            } else {
                *progress
                    .takes
                    .entry((row.prompt_id.clone(), row.condition_id.clone()))
                    .or_default() += 1;
            }
            return progress;
        });
    }

    fn rejected_files(&self) -> HashSet<&str> {
        return self
            .rows
            .iter()
            .filter(|row| return row.event == Event::Rejected)
            .map(|row| return row.file.as_str())
            .collect();
    }
}

/// The manifest always uses `/`, whatever the OS, so the Python side reads the same paths everywhere.
fn file_column(file: &Path) -> String {
    return file
        .iter()
        .map(|part| return part.to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
}

/// Reads every row of a CSV file and opens it for appending. An unfinished last line (no newline: the program
/// stopped mid-write) is cut off; any other bad line is an error.
fn open_csv<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<(Vec<T>, csv::Writer<File>), StoreError> {
    let mut file = OpenOptions::new().read(true).append(true).create(true).open(path)?;
    let mut text = Vec::new();
    file.read_to_end(&mut text)?;
    let complete = text
        .iter()
        .rposition(|&b| return b == b'\n')
        .map_or(0, |newline| return newline + 1);
    if complete < text.len() {
        file.set_len(complete as u64)?;
    }
    let rows = csv::Reader::from_reader(&text[..complete])
        .deserialize()
        .collect::<Result<Vec<T>, csv::Error>>()?;
    let writer = csv::WriterBuilder::new().has_headers(complete == 0).from_writer(file);
    return Ok((rows, writer));
}
