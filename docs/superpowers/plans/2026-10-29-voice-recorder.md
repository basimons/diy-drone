# Voice Recorder Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A controller-driven desktop app (`voice-recorder`) that walks the user through a resumable checklist of
spoken prompts and saves each take as a WAV plus a manifest row, built on a shared `drone-voice` crate that the ground
client will later reuse for live capture.

**Architecture:** `drone-voice` (library, std) holds everything that is not GUI: the plan file, the on-disk store
(WAVs, manifest, removals, progress), the prompt picker, the push-to-talk segmenter and background chunker, take
warnings, and the cpal mic capture. `voice-recorder` is a lib + bin crate: the lib is a pure, tested state machine
(setup screen, take session, background session) that turns button inputs into effects; the bin is a thin eframe/egui
shell that reads gilrs + keyboard, runs the effects against `drone-voice`, and draws.

**Tech Stack:** Rust 2024 workspace; `cpal` (audio in/out), `hound` (WAV), `gilrs` (controller), `eframe`/`egui`
(GUI), `serde` + `toml` (plan), `csv` (manifest, removals), `chrono` (UTC timestamps), `rand` (picker), `tempfile`
(dev-dependency for store tests). Use each crate's current release (`cargo add`).

**Spec:** `docs/superpowers/specs/2026-10-29-voice-recorder-design.md`

## Global Constraints

- House style from `CLAUDE.md`: explicit `return` everywhere (`clippy::implicit_return`), line length 120, few
  comments, enum variant comments on the same line, `[lints] workspace = true` in every new crate.
- Tests live in each crate's `tests/` folder and use the public API only. Test first: write it, watch it fail, implement.
- `scripts/check.sh` (fmt, clippy `-D warnings`, tests) passes after every task; the pre-commit hook runs it.
- Define everything once: prompts, conditions, styles and targets live only in `voice/plan.toml`; phrase prompts are
  derived (`"<wake word>, <command>"`), never listed. Progress is computed from the manifest, never stored.
- No illegal values representable: background cells and prompt cells are different types; no `Result<Result<..>>`,
  no match arms that are only right because of the arms above them. Build values in one expression rather than
  `let mut` + patch-up. Plenty of `debug_assert!` on our own invariants; data read from disk gets real errors.
- Audio: device name contains `"DualSense"`, 48 000 Hz, stored 16-bit mono (channel 0), no resampling/gain/filtering.
  Pre-roll 300 ms, post-roll 300 ms. Background chunks 60 s. No fallback microphone.
- Data root default `voice-data/` (git-ignored), overridable with `--data <dir>`; plan default `voice/plan.toml`,
  overridable with `--plan <file>`. The app never writes the plan file.
- WAV path: `takes/<kind>/<prompt-id>/<condition-id>/<speaker>__<UTC timestamp>__<style>.wav`, relative to the data
  root. Kind dirs: `command`, `wake-word`, `phrase`, `near-miss`, `background`. Timestamp format
  `%Y-%m-%dT%H-%M-%S%.3fZ`. Background files use style `none`.
- Manifest `voice-data/manifest.csv`, header row, columns in this order: `time_utc, session_id, event, file, kind,
  prompt_id, prompt_text, condition_id, style, speaker, duration_ms, sample_rate, channels, device_name,
  capture_volume, peak, warnings`. `event` is `take` or `rejected`; `warnings` is `;`-joined; `kind` is `command`,
  `wake_word`, `phrase`, `near_miss` or `background`.
- Removals `voice-data/removed.csv`: header + `prompt_id, condition_id, time_utc`.
- Commits use `bram.simons@hotmail.com` (repo git config) and end with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

- **The USB cable is pulled mid-session.** The app must not crash or lose saved takes; it shows "microphone lost" and
  disables recording until the controller is back (Task 5 error path, Task 7 manual check).
- **A crash while saving.** A half-written WAV must never be counted and a truncated last manifest line must not stop
  the next start-up (Task 2 tests).
- **The plan file is edited between sessions** (a prompt renamed, a condition dropped). Old manifest rows for unknown
  prompts/conditions are ignored by progress, not errors (Task 2 test).
- **R1 pressed again during the 300 ms post-roll** (fast speaker). The previous take is completed and saved; the new
  one starts with its own pre-roll (Task 4 test).
- **Names that aren't file-safe** (`"Hommel, flip"`, `"flat / tired"`, a speaker called `"Bram S."`) always produce
  safe, distinct path parts (Task 1 and Task 2 tests).

---

### Task 1: `drone-voice` crate and the plan file

**Files:**
- Modify: `Cargo.toml` (add `voice` to `members`; add `drone-voice = { path = "voice" }` to `[workspace.dependencies]`)
- Create: `voice/Cargo.toml`, `voice/src/lib.rs`, `voice/src/plan.rs`, `voice/plan.toml`
- Test: `voice/tests/plan.rs`

**Interfaces:**
- Produces:
  - `pub enum PromptKind { Command, WakeWord, Phrase, NearMiss }` with `fn as_str(self) -> &'static str`
    (`command`, `wake_word`, `phrase`, `near_miss`) and `fn dir(self) -> &'static str` (`command`, `wake-word`,
    `phrase`, `near-miss`).
  - `pub struct Condition { pub id: String, pub label: String }`
  - `pub struct PromptCell { pub kind: PromptKind, pub prompt_id: String, pub text: String, pub condition_id: String,
    pub target: u32 }` (`Clone, Debug, PartialEq, Eq, Hash`)
  - `pub struct BackgroundCell { pub condition_id: String, pub target: Duration }` (`Clone, Debug, PartialEq, Eq`)
  - `pub struct Plan` with `pub fn parse(text: &str) -> Result<Plan, PlanError>`,
    `pub fn load(path: &Path) -> Result<Plan, PlanError>`, `pub fn conditions(&self) -> &[Condition]`,
    `pub fn styles(&self) -> &[String]`, `pub fn prompt_cells(&self) -> &[PromptCell]`,
    `pub fn background_cells(&self) -> &[BackgroundCell]`.
  - `pub fn slug(text: &str) -> String`: lowercase; ASCII letters/digits kept; every run of anything else becomes one
    `-`; leading/trailing `-` trimmed.
  - `pub enum PlanError` (`Display` + `Error`): `Toml(String)`, `Io(String)`, `UnknownCondition(String)`,
    `DuplicatePromptId(String)`, `Empty(&'static str)` (no styles / no conditions / a kind with no prompts),
    `ZeroTarget(&'static str)`.

Plan file shape (`voice/plan.toml`, content from the spec's vocabulary, conditions, styles and targets table):

```toml
styles = ["normal", "fast", "slow", "quiet", "loud", "flat / tired", "urgent", "annoyed"]

[[conditions]]
id = "quiet-room"
label = "Quiet room"
# … tv-music, fan, moving-sticks, outdoors-calm, outdoors-windy, drone-nearby

[command]
target = 12
conditions = ["quiet-room", "moving-sticks", "outdoors-windy", "drone-nearby"]
prompts = ["stop", "land", "come back", "takeoff", "hover", "forward", "back", "left", "right", "up", "down",
           "turn left", "turn right", "flip", "spin", "circle", "follow me", "photo", "record"]

[wake_word]
target = 40
conditions = [ … same four … ]
prompt = "Hommel"

[phrase]            # prompts derived: "Hommel, stop", "Hommel, land", …
target = 5
conditions = ["quiet-room", "outdoors-windy"]

[near_miss]
target = 5
conditions = ["quiet-room", "moving-sticks"]
prompts = ["hommels", "hummel", "hamel", "Rommel", "hemel", "om elf", "mommel", "lift", "flipper", "back off",
           "landen", "any short sentence"]

[background]
minutes = 5
conditions = [ … all seven … ]
```

Prompt ids are `slug(text)` and must be unique across the whole plan (a phrase `hommel-flip` can't collide with
anything else). Cells are in plan order: kind order as above, then prompt order, then condition order.

- [ ] **Step 1: Write the failing tests** in `voice/tests/plan.rs`:
  - `the_shipped_plan_parses_to_1382_takes`: `Plan::load("plan.toml")` (path relative to `CARGO_MANIFEST_DIR`);
    sum of `target` over `prompt_cells()` == 1382; `background_cells().len() == 7`, each `target == 300 s`;
    `conditions().len() == 7`; `styles().len() == 8`.
  - `phrases_are_derived_from_wake_word_and_commands`: in the shipped plan, a `PromptCell` with
    `kind == Phrase`, `text == "Hommel, turn left"`, `prompt_id == "hommel-turn-left"` exists for `quiet-room`; the
    number of phrase cells == 19 × 2.
  - `slug_makes_file_safe_names`: `slug("turn left") == "turn-left"`, `slug("Hommel, flip") == "hommel-flip"`,
    `slug("flat / tired") == "flat-tired"`, `slug("Bram S.") == "bram-s"`, `slug("  Rommel!! ") == "rommel"`.
  - `rejects_an_unknown_condition`, `rejects_duplicate_prompt_ids` (command `"flip"` and near-miss `"Flip!"`),
    `rejects_empty_styles`, `rejects_a_zero_target`, `rejects_bad_toml`: each `Plan::parse` of a small inline plan
    returns the matching `PlanError` variant.
- [ ] **Step 2: Run** `cargo test -p drone-voice --test plan`. Expected: FAIL (crate/functions missing).
- [ ] **Step 3: Implement** `voice/Cargo.toml` (name `drone-voice`, deps `serde` with `derive`, `toml`), `lib.rs`
  re-exporting the public items, `plan.rs` (serde structs for the raw file, then validate into `Plan`), and
  `voice/plan.toml`.
- [ ] **Step 4: Run** `cargo test -p drone-voice --test plan` → PASS; then `scripts/check.sh` → passes.
- [ ] **Step 5: Commit** `drone-voice: plan file (prompts, conditions, styles, targets)`.

### Task 2: Store (WAVs, manifest, removals, progress)

**Files:**
- Create: `voice/src/store.rs`, `voice/src/wav.rs`
- Modify: `voice/src/lib.rs`, `voice/Cargo.toml` (`hound`, `csv`, `chrono`; dev: `tempfile`)
- Test: `voice/tests/store.rs`

**Interfaces:**
- Consumes: `PromptCell`, `BackgroundCell`, `PromptKind`, `slug` (Task 1).
- Produces:
  - `pub struct MicInfo { pub device_name: String, pub sample_rate: u32, pub channels: u16 }` (here, reused by Task 5;
    `channels` is what the device delivers, the stored WAV is always mono).
  - `pub enum Warning { Clipped, VeryQuiet, TooShort, TooLong }` with `fn as_str(self) -> &'static str`
    (`clipped`, `very_quiet`, `too_short`, `too_long`). (Produced by Task 4's analysis; defined here because the
    store writes it.)
  - `pub enum Clip<'a> { Prompt(&'a PromptCell), Background(&'a BackgroundCell) }`
  - `pub struct NewTake<'a> { pub clip: Clip<'a>, pub samples: &'a [i16], pub style: Option<&'a str>,
    pub speaker: &'a str, pub session_id: &'a str, pub time: DateTime<Utc>, pub mic: &'a MicInfo,
    pub capture_volume: Option<&'a str>, pub warnings: &'a [Warning] }`
  - `pub struct Store` with `pub fn open(root: &Path) -> Result<Store, StoreError>`,
    `pub fn save(&mut self, take: NewTake<'_>) -> Result<PathBuf, StoreError>` (returns the path relative to root),
    `pub fn reject(&mut self, file: &Path, time: DateTime<Utc>) -> Result<(), StoreError>`,
    `pub fn remove(&mut self, cell: &PromptCell, time: DateTime<Utc>) -> Result<(), StoreError>`,
    `pub fn is_removed(&self, cell: &PromptCell) -> bool`, `pub fn progress(&self) -> Progress`,
    `pub fn last_speaker(&self) -> Option<&str>`, `pub fn root(&self) -> &Path`.
  - `pub struct Progress` with `pub fn takes(&self, cell: &PromptCell) -> u32` and
    `pub fn recorded(&self, cell: &BackgroundCell) -> Duration`.
  - `pub fn read_wav(path: &Path) -> Result<(Vec<i16>, u32), StoreError>` (samples, sample rate), for playback.
  - `pub enum StoreError` (`Display` + `Error`): `Io(String)`, `Csv(String)`, `Wav(String)`, `UnknownFile(PathBuf)`.

Behaviour the tests pin:
- `save` writes the WAV to `<path>.part`, renames it into place, then appends the manifest row and flushes; returns
  only after both. Peak = max `|sample|`; `duration_ms` from samples / `mic.sample_rate`. WAV is mono 16-bit at
  `mic.sample_rate`. Speaker and style go through `slug` in the file name (raw text in the manifest).
- `reject` moves the WAV to `rejected/<same relative path>` and appends a `rejected` row for that `file`;
  `UnknownFile` if the manifest has no `take` row for it.
- Progress counts `take` rows whose file has no `rejected` row, keyed by `(prompt_id, condition_id)`; background sums
  `duration_ms` keyed by condition. Rows for prompts/conditions not in the current plan are simply never asked for.
- `open` on a missing root creates it and writes the CSV headers. A last manifest line that doesn't parse (crash
  mid-append) is ignored; `.part` files are never counted. Any other bad line is a `Csv` error.

- [ ] **Step 1: Write the failing tests** in `voice/tests/store.rs` (each in a `tempfile::tempdir()`; a small helper
  builds a `PromptCell` for `flip` / `quiet-room` and a `MicInfo { "DualSense Wireless Controller", 48000, 2 }`):
  - `a_saved_take_is_a_mono_wav_at_its_documented_path`: save 4 800 samples at `2026-10-29T14:03:12.345Z`, style
    `"flat / tired"`, speaker `"Bram S."` → returned path is
    `takes/command/flip/quiet-room/bram-s__2026-10-29T14-03-12.345Z__flat-tired.wav`; `read_wav` gives the same
    samples at 48 000.
  - `progress_counts_takes_and_survives_reopening`: 3 saves → `progress().takes(&cell) == 3`; drop, `Store::open`
    again → still 3; `last_speaker() == Some("Bram S.")`.
  - `a_rejected_take_stops_counting_and_moves_aside`: save 2, reject the first → `takes == 1`, original path gone,
    `rejected/<path>` exists.
  - `rejecting_an_unknown_file_is_an_error` → `StoreError::UnknownFile`.
  - `background_progress_sums_durations`: two background saves of 48 000 and 24 000 samples → `recorded == 1.5 s`.
  - `removed_cells_persist`: `remove(&cell)` → `is_removed` true, and still true after reopening; `removed.csv` has a
    header and one row `flip,quiet-room,<time>`.
  - `a_truncated_last_manifest_line_is_ignored`: after 2 saves append `"2026-10-29T1"` (no newline) to
    `manifest.csv` → reopen OK, `takes == 2`; a following `save` still works and counts (3).
  - `part_files_are_not_takes`: a stray `takes/…/x.wav.part` file doesn't change progress.
  - `manifest_rows_for_prompts_no_longer_in_the_plan_are_harmless`: save for a cell `old-word`, then progress for
    `flip` is 0 and nothing errors.
  - `manifest_has_the_documented_columns`: first line of `manifest.csv` equals the header from Global Constraints;
    a saved row has `event == "take"`, `kind == "command"`, `peak` as the max abs sample, `warnings` `"clipped;too_long"`
    when given those two.
- [ ] **Step 2: Run** `cargo test -p drone-voice --test store` → FAIL.
- [ ] **Step 3: Implement** `store.rs` and `wav.rs`. The store keeps the parsed rows in memory (a few thousand rows)
  and the open manifest file handle for appends.
- [ ] **Step 4: Run** `cargo test -p drone-voice --test store` → PASS; `scripts/check.sh` → passes.
- [ ] **Step 5: Commit** `drone-voice: store for takes, manifest, removals and progress`.

### Task 3: Prompt picker

**Files:**
- Create: `voice/src/picker.rs`
- Modify: `voice/src/lib.rs`, `voice/Cargo.toml` (`rand`)
- Test: `voice/tests/picker.rs`

**Interfaces:**
- Consumes: `Plan`, `PromptCell`, `PromptKind` (Task 1); `Progress`, `Store::is_removed` (Task 2).
- Produces:
  - `pub struct Selection { pub kinds: Vec<PromptKind>, pub conditions: Vec<String>, pub length: usize }`
  - `pub struct QueuedTake { pub cell: PromptCell, pub style: String }` (`Clone, Debug, PartialEq`)
  - `pub fn pick(plan: &Plan, progress: &Progress, is_removed: impl Fn(&PromptCell) -> bool, selection: &Selection,
    rng: &mut impl Rng) -> Vec<QueuedTake>`

Algorithm (not determined by the tests alone):

```
left[cell] = cell.target - progress.takes(cell)  (saturating), for eligible cells:
    kind in selection.kinds, condition in selection.conditions, !is_removed(cell), left > 0
repeat selection.length times:
    candidates = cells with left > 0 whose prompt_id != previous pick's prompt_id
    if candidates is empty: candidates = cells with left > 0
    if candidates is empty: stop
    pick one at random weighted by left; left -= 1; style = random from plan.styles()
```

- [ ] **Step 1: Write the failing tests** in `voice/tests/picker.rs`, using a small inline plan and a
  `StdRng::seed_from_u64` over many seeds where randomness matters:
  - `only_selected_kinds_and_conditions_come_up`
  - `removed_and_complete_cells_never_come_up` (progress built by saving takes into a temp `Store`)
  - `never_asks_more_than_a_cell_still_needs`: a cell needing 2, length 50 → that cell appears exactly 2 times.
  - `the_same_prompt_never_comes_twice_in_a_row_when_there_is_a_choice`: 2 prompts × 10 left, length 20, 100 seeds.
  - `stops_early_when_everything_is_done`: total left 5, length 120 → 5 entries.
  - `emptier_cells_come_up_more`: cell A 20 left, cell B 2 left, length 5, over 200 seeds A is picked more often
    than B in total.
  - `every_take_gets_a_style_from_the_plan`.
- [ ] **Step 2: Run** `cargo test -p drone-voice --test picker` → FAIL.
- [ ] **Step 3: Implement** `pick` as above.
- [ ] **Step 4: Run** → PASS; `scripts/check.sh` → passes.
- [ ] **Step 5: Commit** `drone-voice: prompt picker`.

### Task 4: Push-to-talk segmenter, background chunker, take warnings

**Files:**
- Create: `voice/src/segment.rs`
- Modify: `voice/src/lib.rs`
- Test: `voice/tests/segment.rs`

**Interfaces:**
- Consumes: `Warning`, `PromptKind`.
- Produces:
  - `pub struct PushToTalk` with `pub fn new(sample_rate: u32) -> PushToTalk` (pre/post roll 300 ms, from Global
    Constraints, as constants), `pub fn press(&mut self) -> Option<Segment>` (returns the previous take if it was still
    in its post-roll), `pub fn release(&mut self)`, `pub fn feed(&mut self, samples: &[i16]) -> Option<Segment>`.
  - `pub struct Segment { pub samples: Vec<i16>, pub held: Duration }` (`held` = audio time between press and release).
  - `pub struct Chunker` with `pub fn new(sample_rate: u32, limit: Duration) -> Chunker` (chunk length 60 s),
    `pub fn feed(&mut self, samples: &[i16]) -> Vec<Vec<i16>>` (full chunks; stops taking audio at `limit`),
    `pub fn is_done(&self) -> bool`, `pub fn finish(self) -> Option<Vec<i16>>` (the partial last chunk, if any).
  - `pub fn warnings(kind: PromptKind, segment: &Segment) -> Vec<Warning>`: `Clipped` if peak ≥ 32 000;
    `VeryQuiet` if peak < 1 000; `TooShort` if `held` < 300 ms; `TooLong` if `held` > 4 s and kind is `Command`,
    `WakeWord` or `Phrase`.

Time is counted in samples, never with a clock, so tests are exact. A `release()` with no press is ignored (R1 may be
held when the app starts). A `press()` while already held is ignored.

- [ ] **Step 1: Write the failing tests** in `voice/tests/segment.rs` (sample rate 1 000 Hz for readable numbers, so
  300 ms = 300 samples; feed ramps so positions are checkable):
  - `a_take_starts_300_ms_before_the_press_and_ends_300_ms_after_release`: feed 1 000, press, feed 500, release,
    feed 1 000 → one `Segment` of 1 100 samples whose first sample is stream sample 700; `held == 500 ms`.
  - `less_than_300_ms_before_the_first_press_gives_a_shorter_pre_roll`.
  - `no_segment_until_the_post_roll_is_complete`: after release, feeding 299 samples returns `None`, the 300th
    returns the segment.
  - `pressing_again_during_post_roll_completes_the_previous_take`: release, feed 100, `press()` returns the previous
    segment (post-roll 100); the next take's segment starts 300 samples before this press.
  - `a_release_without_a_press_is_ignored`, `a_second_press_while_held_is_ignored`.
  - `the_chunker_cuts_60_s_chunks_and_stops_at_the_limit` (1 000 Hz, limit 150 s → two 60 000 chunks from `feed`,
    `is_done` after 150 000 samples, `finish` gives 30 000).
  - `warnings_for_clipping_quiet_short_and_long_takes`, including `TooLong` not raised for `NearMiss`.
- [ ] **Step 2: Run** `cargo test -p drone-voice --test segment` → FAIL.
- [ ] **Step 3: Implement** with a ring buffer of the last 300 ms while idle.
- [ ] **Step 4: Run** → PASS; `scripts/check.sh` → passes.
- [ ] **Step 5: Commit** `drone-voice: push-to-talk segmenter, background chunker, take warnings`.

### Task 5: Mic capture (cpal) and CI

**Files:**
- Create: `voice/src/mic.rs`, `voice/tests/mic_hardware.rs`
- Modify: `voice/src/lib.rs`, `voice/Cargo.toml` (`cpal`), `.github/workflows/ci.yml` (add `libasound2-dev` to the
  `apt-get install` line and its comment), `README.md` (ALSA headers note next to the `libudev` one)

**Interfaces:**
- Consumes: `MicInfo` (Task 2).
- Produces:
  - `pub struct Mic` with `pub fn open() -> Result<Mic, MicError>` (first input device whose name contains
    `"DualSense"`, a 48 000 Hz config, i16 or f32 samples converted to i16, channel 0 only),
    `pub fn info(&self) -> &MicInfo`, `pub fn receiver(&self) -> &std::sync::mpsc::Receiver<MicEvent>`.
    Dropping `Mic` stops the stream.
  - `pub enum MicEvent { Samples(Vec<i16>), Lost(String) }` (`Lost` from cpal's stream error callback, e.g. cable
    pulled).
  - `pub enum MicError` (`Display` + `Error`): `NotFound`, `No48kHz(String)`, `Backend(String)`.
  - `pub fn capture_volume(info: &MicInfo) -> Option<String>`: on Linux, run `amixer -c Controller sget Mic` and
    return the first `[NN%]` value; `None` on any failure or other OS.

- [ ] **Step 1: Write the hardware test** `voice/tests/mic_hardware.rs`, marked `#[ignore]` (needs the DualSense on
  USB): `the_dualsense_delivers_48_khz_audio`: `Mic::open()` OK, `info().sample_rate == 48000`, within 1 s at least
  40 000 samples arrive as `MicEvent::Samples`. Print `info()` and `capture_volume` so the channel count the
  DualSense really delivers can be noted.
- [ ] **Step 2: Run** `cargo test -p drone-voice --test mic_hardware -- --ignored` → FAIL (no `Mic`).
- [ ] **Step 3: Implement** `mic.rs`. The cpal callback must not block: convert and `send` only.
- [ ] **Step 4: Run** the ignored test with the controller plugged in → PASS. Note the printed channel count and
  volume in the commit message. Unplug the controller and run again → `MicError::NotFound`. `scripts/check.sh`
  → passes (the ignored test doesn't run in CI).
- [ ] **Step 5: Commit** `drone-voice: DualSense mic capture; CI installs ALSA headers`.

### Task 6: Recorder state machine (`voice-recorder` lib)

**Files:**
- Modify: `Cargo.toml` (add `voice-recorder` to `members`)
- Create: `voice-recorder/Cargo.toml`, `voice-recorder/src/lib.rs`, `voice-recorder/src/setup.rs`,
  `voice-recorder/src/session.rs`
- Test: `voice-recorder/tests/setup.rs`, `voice-recorder/tests/session.rs`

**Interfaces:**
- Consumes: `Plan`, `PromptKind`, `PromptCell`, `BackgroundCell`, `Selection`, `QueuedTake` (Tasks 1, 3).
- Produces:
  - `pub enum Button { R1, Cross, Circle, Triangle, Square, Options, Up, Down, Left, Right }`
  - `pub enum Input { Down(Button), Up(Button) }`
  - `pub enum Effect { StartTake, StopTake, StartBackground, StopBackground, Reject(PathBuf), Play(PathBuf),
    Remove(PromptCell), End }`
  - `pub struct Setup` — `pub fn new(plan: &Plan, speaker: String) -> Setup`; fields read by the GUI through getters
    (`kinds()`, `conditions()`, `background()`, `length()`, `cursor()`); `pub fn handle(&mut self, button: Button)`
    (D-pad moves the cursor over rows / changes length by 10 within 10..=500, ✕ toggles the row under the cursor);
    `pub fn start(&self) -> Option<Start>`.
  - `pub enum Start { Takes(Selection), Background(BackgroundCell) }`
  - `pub struct TakeSession` — `pub fn new(queue: Vec<QueuedTake>) -> TakeSession`,
    `pub fn current(&self) -> Option<&QueuedTake>` (`None` = finished), `pub fn position(&self) -> (usize, usize)`,
    `pub fn handle(&mut self, input: Input, now: Instant) -> Vec<Effect>`, `pub fn tick(&mut self, now: Instant)
    -> Vec<Effect>`, `pub fn saved(&mut self, file: PathBuf)`, `pub fn is_paused(&self) -> bool`,
    `pub fn summary(&self) -> Vec<(String, u32)>` (condition id → takes kept this session).
  - `pub struct BackgroundSession` — `pub fn new(cell: BackgroundCell) -> BackgroundSession`,
    `pub fn handle(&mut self, input: Input) -> Vec<Effect>`, `pub fn chunk_saved(&mut self, file: PathBuf,
    length: Duration)`, `pub fn stopped(&mut self)` (the chunker reached its limit), `pub fn is_recording(&self)
    -> bool`, `pub fn recorded(&self) -> Duration`.

Rules the tests pin:
- Setup: ticking background unticks every prompt kind and vice versa; `start()` is `None` when nothing is ticked, when
  prompt kinds are ticked but no condition, or when background is ticked with other than exactly one condition.
  Defaults: all prompt kinds ticked, no condition ticked, length 120.
- TakeSession: `R1` down → `[StartTake]`, up → `[StopTake]`; `saved(file)` advances to the next queued take and
  remembers `file` as the last take. `Circle` with a last take → `[Reject(file)]` and that take's prompt is current
  again; without one → `[]`. `Triangle` → `[Play(last)]`. `Square` down then up within 1 s → skip: every queued entry
  of the current cell is dropped, no effect. `Square` held: `tick` at ≥ 1 s after the down → `[Remove(cell)]` and its
  entries dropped; the later `Square` up does nothing. `Options` toggles pause; while paused only `Options` (resume)
  and `Cross` (`[End]`) do anything. While R1 is held, other buttons are ignored.
- BackgroundSession: `R1` down toggles `[StartBackground]` / `[StopBackground]`; `Circle` → `[Reject(last chunk)]`
  and subtracts its length from `recorded`; `Triangle` → `[Play(last chunk)]`; `Square` does nothing.

- [ ] **Step 1: Write the failing tests** in `voice-recorder/tests/setup.rs` and `session.rs`, one test per rule
  above (names like `skip_drops_the_cell_for_this_session_only`, `holding_square_for_a_second_removes_the_cell`,
  `redo_rejects_the_last_take_and_asks_again`, `paused_ignores_recording_buttons`,
  `background_is_its_own_session_type`).
- [ ] **Step 2: Run** `cargo test -p voice-recorder` → FAIL.
- [ ] **Step 3: Implement** `voice-recorder/Cargo.toml` (lib + `[[bin]] name = "voice-recorder"`, depends on
  `drone-voice`), `setup.rs`, `session.rs`. No I/O, no clocks: `now` is passed in.
- [ ] **Step 4: Run** → PASS; `scripts/check.sh` → passes.
- [ ] **Step 5: Commit** `voice-recorder: setup and session state machines`.

### Task 7: GUI shell and end-to-end run

**Files:**
- Create: `voice-recorder/src/main.rs`, `voice-recorder/src/app.rs` (eframe `App`), `voice-recorder/src/input.rs`
  (gilrs + egui keys → `Input`), `voice-recorder/src/playback.rs` (cpal default output)
- Modify: `voice-recorder/Cargo.toml` (`eframe`, `gilrs` (workspace version as the client), `cpal`, `chrono`)

**Interfaces:**
- Consumes: everything above.
- Produces: the `voice-recorder` binary: `cargo run -p voice-recorder [-- --data <dir> --plan <file>]`.

Wiring the implementer must not miss:
- Button map (gilrs → `Button`): `RightTrigger` → R1, `South` → Cross, `East` → Circle, `North` → Triangle, `West` →
  Square, `Start` → Options, `DPad*` → Up/Down/Left/Right. Keyboard: Space → R1, Enter → Cross, Backspace → Circle,
  P → Triangle, S → Square, Esc → Options, arrows → D-pad.
- Each frame: drain `Mic::receiver()`; feed samples to the active `PushToTalk` (or `Chunker`); on a finished `Segment`
  compute `warnings`, `Store::save` with the current `QueuedTake`, call `TakeSession::saved`. Run each `Effect`
  against `Store` / `playback`. Call `tick(Instant::now())`; `request_repaint` while a session runs.
- `session_id` = UTC start time of the session (same format as file timestamps). Speaker defaults to
  `Store::last_speaker()`, else `"speaker"`, editable as text on the setup screen.
- Setup screen per the spec and mockup: conditions with progress (`kept / target` summed over that condition's cells),
  kinds, length, overall progress, Start disabled with a visible reason when `Mic::open` failed, no controller is
  connected, or `Setup::start()` is `None`.
- Take screen: `take n / N · condition label`, big prompt text (near-miss `any short sentence` shows "Say any short
  sentence, Dutch or English"), style, a level meter from the last block's peak, the last take's waveform and its
  warnings, button hints.
- `MicEvent::Lost` → stop any recording, show "Microphone lost — reconnect the controller", retry `Mic::open` once a
  second; saved takes are untouched.
- Summary screen from `TakeSession::summary()` / `BackgroundSession::recorded()`, then back to setup.

- [ ] **Step 1: Implement** the files above (the logic is already tested in Tasks 1–6; this task is wiring).
- [ ] **Step 2: Run** `scripts/check.sh` → passes.
- [ ] **Step 3: Run it end to end** with the controller on USB and `--data` pointing at a scratch directory:
  setup → tick quiet room → record 5 takes with R1 → ○ redo one → △ play one → □ skip one → hold □ to remove one →
  Options + ✕ to end. Check: 5 WAVs at the documented paths (+1 in `rejected/`), `manifest.csv` rows match,
  `removed.csv` has the removed cell, restarting the app shows the progress and doesn't offer the removed cell.
  Then a 1-minute background session (target lowered with a scratch `--plan`) stops by itself. Then pull the USB
  cable mid-session: no crash, message shown, recording resumes after plugging back in.
- [ ] **Step 4: Commit** `voice-recorder: GUI for recording the voice dataset`.

### Task 8: Docs

**Files:**
- Modify: `README.md` (rows for `voice/` and `voice-recorder/` in the Projects table, a "Recording voice commands"
  section with the run command and where data goes, Status line)
- Create: `voice/README.md` (what the crate offers; plan file format; data layout and manifest columns, i.e. the
  contract for the Python training side)

- [ ] **Step 1: Write the docs.** Explain terms for a newcomer (push-to-talk, wake word, augmentation).
- [ ] **Step 2: Run the README's commands** exactly as written (scratch `--data`), confirm they work.
- [ ] **Step 3: Commit** `Docs: voice recorder and drone-voice`.
- [ ] **Step 4: Before the pull request:** run an independent review agent over the branch diff (bugs plus awkward
  shapes per `CLAUDE.md`), fix what it finds, then open the PR against `main` (squash merge).
