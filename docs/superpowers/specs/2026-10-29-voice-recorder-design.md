# Voice command recorder: design

Date: 2026-10-29

## Why

We want to give the drone spoken commands ("Hommel, flip") as well as stick input. To do that we need to train a small
speech model, and to train it we need a lot of recordings of the commands: many takes, said in different ways, in
different kinds of noise, recorded with the exact microphone and capture code that will be used in flight.

This design covers only **gathering those recordings**: a small GUI app that prompts for a word, records it while R1 is
held, and keeps a checklist so recording can be spread over many sessions (indoors tonight, outdoors at the weekend).
Training and live recognition come later and are only described here as far as the recordings must suit them.

## Goals

- A desktop GUI app, `voice-recorder`, fully usable from the PS5 controller (DualSense).
- A checklist of everything to record (prompt × condition, each needing a number of takes), with progress that survives
  restarts; every take is saved the moment it is accepted.
- At start-up, the user ticks which conditions are possible right now; the app only asks for those.
- Variety for free: the app assigns a speaking style to each take and shuffles prompts.
- The audio capture code (device, format, push-to-talk timing) lives in a shared library crate that the ground client
  will later use for live recognition, so training data and flight audio come through the same code.
- Data on disk in a simple, documented layout with a manifest that Python training code can read.

## Non-goals (for now)

- Training a model, or any Python code. Only the data contract with it is fixed here.
- Live recognition in the client, or the wake-word detector.
- Recording over Bluetooth (Linux does not get the DualSense mic over Bluetooth; it works over USB only).
- Any audio processing (noise reduction, gain, normalising) at record time. Augmentation (mixing in noise) happens at
  training time in Python.

## Anti-goals (would be a failure even if it works)

- The recorder captures audio differently from how the client will (other device, rate, channel handling, trimming).
- A crash or a closed window loses accepted takes or progress.
- Recordings end up in git.
- Arm / disarm by voice. Arming the motors always stays a deliberate button action.

## Hardware facts (verified on the dev laptop)

- The DualSense over USB shows up as gamepad `054c:0ce6` and as ALSA capture card "DualSense Wireless Controller"
  (`USB Audio`). Over Bluetooth there is no mic on Linux, so in the field the controller is USB-cabled to the ground
  laptop.
- The mic sits in the controller, so it is always ~30–50 cm from the mouth, in the user's hands, next to stick and button
  noise. Props are distant background; wind on the mic outdoors is the main noise.
- In flight the thumbs are on the sticks and the index finger is free for R1, which becomes the push-to-talk button.

## Vocabulary

| Kind | Prompts |
|---|---|
| Wake word | `Hommel` (Dutch: bumblebee) |
| Commands, safety | `stop`, `land`, `come back` |
| Commands, basics | `takeoff`, `hover` |
| Commands, movement | `forward`, `back`, `left`, `right`, `up`, `down`, `turn left`, `turn right` |
| Commands, tricks | `flip`, `spin`, `circle`, `follow me`, `photo`, `record` |
| Phrases | `Hommel, <command>` for every command above |
| Near-misses | `hommels`, `hummel`, `hamel`, `Rommel`, `hemel`, `om elf`, `mommel`, `lift`, `flipper`, `back off`, `landen`, plus "say any short sentence (Dutch or English)" |
| Background | long hands-free stretches of ordinary sound, no prompt |

"Hommel" has soft *m* sounds that blur in noise, which is why it gets extra takes and why the near-miss list leans on
words that sound like it.

The two ways of talking to the drone later, which the data must serve:

- **Push-to-talk:** hold R1, say only the command. Model answers "which command (or none)?".
- **Wake word:** an always-on detector listens for "Hommel"; on a hit, the next ~2 s go to the same command model.

## Conditions and styles

**Conditions** (where you are; ticked at start-up): quiet room · TV / music on · fan / kitchen hood running · while
moving the sticks · outdoors calm · outdoors windy · drone running nearby.

"While moving the sticks" means: actively move the sticks and press buttons while speaking, because in flight that noise
sits right next to the mic.

**Speaking styles** (assigned by the app per take, at random, shown on screen): normal · fast · slow · quiet · loud ·
flat / tired · urgent · annoyed.

## Checklist

The checklist is a grid of **cells**: one cell per (prompt, condition), each with a target number of takes. Progress is
counted from the takes on disk, never stored separately.

Targets (our call, trimmed with the user to a few hours of recording). The trick that keeps this short: noise that can be faked well (TV, music, a fan) is not recorded under every take.
Instead it is recorded once as background and mixed into clean takes at training time (augmentation). Takes are only
recorded live in the conditions that can't be faked well: the voice itself changes (outdoors, shouting over wind) or
the noise sits on the mic (sticks, wind).

| Kind | Takes per cell | Conditions |
|---|---|---|
| Commands | 12 | quiet room, while moving sticks, outdoors windy, drone running nearby |
| Hommel | 40 | same four |
| Phrases | 5 | quiet room, outdoors windy |
| Near-misses | 5 | quiet room, while moving sticks |
| Background | 5 minutes | all seven (hands-free, costs no effort) |

Total: 19 × 12 × 4 + 40 × 4 + 19 × 5 × 2 + 12 × 5 × 2 = 912 + 160 + 190 + 120 = **1,382 takes, about 2–3 hours** at
5–8 s per take, spread over sessions, plus ~35 minutes of hands-free background. If the trained model struggles on
some word or condition, raising a target in `plan.toml` simply puts those cells back on the checklist.

Each kind in `plan.toml` lists the conditions it applies to; targets and condition lists change without code changes.

## Screens and controls

All screens work with the controller only. The keyboard mirrors the buttons (our call: Space = R1, Enter = ✕,
Backspace = ○, P = △, S = □, Esc = OPTIONS) so the app can be developed and tested without a controller.

### Screen 1: start-up ("Where are you right now?")

- Speaker name (defaults to the last one used), so other people's voices can be added later.
- List of conditions with a checkbox each and their progress (`412 / 600 takes`). D-pad moves, ✕ ticks.
- Which kinds to record: commands · Hommel · phrases · near-misses · background.
- Session length in takes (D-pad left/right, default 120 ≈ 15 min).
- Background is not mixed into a take session (see "Background sessions" below): ticking it alongside other kinds is
  not possible; it is its own session type.
- Overall progress line.
- ✕ on "Start" begins the session.
- A warning (and Start disabled) if no DualSense mic or controller is found.

### Screen 2: one take

- Top line: take n / N, current condition.
- Big prompt text ("turn left"), and the style for this take ("fast, a bit annoyed"). For near-miss "free sentence"
  prompts, the text says so. For phrases, the full phrase ("Hommel, flip").
- Live level meter while recording; waveform of the last take after it.
- Controls:
  - **Hold R1**: record. Release ends the take.
  - **○**: redo. Discards the last take and asks the same prompt again.
  - **△**: play back the last take (laptop speakers).
  - **□ tap**: skip. Not now; this prompt is dropped from this session only and comes back in later sessions.
  - **□ hold ~1 s**: remove. Never ask this cell again (written to the removals file, see below; undo by editing it).
  - **OPTIONS**: pause menu: resume / end session.
- After release the take is saved immediately and the next prompt appears; ○ and △ act on the take just made.
- Warnings shown on the take (not blocking): clipped (peak at full scale), very quiet, too short (released before any
  speech was likely, < 0.3 s held), too long (> 4 s for commands/phrases).

### Background sessions

Background is its own session type, not an item in the take queue (our call). The user picks exactly one condition,
then R1 **toggles** recording (press to start, press to stop), since it runs hands-free for minutes. The screen shows
elapsed time and the minutes still needed for that condition; recording also stops by itself when that target is
reached. Audio is written in 60 s chunks, each its own WAV with its own `take` row, so a crash loses at most the last
chunk. Progress counts the summed `duration_ms` of the chunks. No pre/post roll (it's continuous). ○ rejects only the
last chunk; △ plays the last chunk; □ is not used.

### End of session

A summary: takes made, per condition, what's left overall. Back to screen 1 or quit.

## How the app picks prompts

1. Eligible cells: kind ticked, condition ticked, not removed, below target, not skipped this session.
2. Build a queue of the session length, weighting each cell by how many takes it still needs (emptiest first, but not
   strictly in order), shuffled so the same prompt never comes twice in a row when there is any alternative.
3. Each queued take gets a random style.
4. When the eligible cells run out, the session ends early with the summary.

## Audio capture (the shared part)

- Device: the ALSA/cpal input whose name contains "DualSense". No fallback to another mic in the recorder (a take from
  the laptop mic would poison the data); the recorder refuses to start without it.
- Format: whatever the device natively gives at 48 kHz, stored as 48 kHz, 16-bit, mono WAV. If the device delivers more
  than one channel, take channel 0 (our call; the builder should check what the DualSense actually provides and note it).
  No resampling, gain or filtering in Rust; downsampling to 16 kHz happens in training.
- Push-to-talk timing: a rolling pre-roll buffer of 300 ms, so the take starts 300 ms before R1 went down; recording
  continues 300 ms after R1 is released.
- The OS capture volume for the DualSense is not touched by the app, but its current value is logged in the manifest if
  it can be read (helps explain level differences between sessions).

This capture code (device selection, format, pre/post roll, push-to-talk segmentation) is the part the client will reuse
for live recognition, so it goes in the shared crate, not in the recorder.

Shape (our call): the crate exposes one continuous capture stream from the mic, and the push-to-talk segmenter sits on
top of it. Later the client's wake-word mode will read the same stream and cut its own fixed window after a "Hommel"
hit; that windowing is client work, not part of this design. Push-to-talk clips serve both modes for training: the
training code crops and pads them into whatever window the wake-word detector uses, and the background recordings
provide the long "nothing was said" stretches it needs. So the recorder has no separate hands-free flow for Hommel or
phrases.

## Code layout

| Folder | Crate | What |
|---|---|---|
| `voice/` | `drone-voice` (library) | Mic capture, push-to-talk segmenting, WAV writing, plan file + manifest reading/writing, the prompt picker. No GUI. |
| `voice-recorder/` | `voice-recorder` (binary) | The egui GUI, the controller mapping, the session flow. Depends on `drone-voice`. |
| `voice-training/` | (Python, later) | Training code. Not part of this work. |

- Libraries: `cpal` (audio), `hound` (WAV), `gilrs` (controller, already used by the client), `eframe`/`egui` (GUI),
  `serde` + `toml` (plan file), `csv` (manifest). Builder may swap one if there's a good reason.
- gilrs naming: the DualSense **R1** is `Button::RightTrigger` (R2 is `RightTrigger2`).
- Linux builds need ALSA headers (`alsa-lib-devel` / `libasound2-dev`); CI needs that package added. Add to README.
- Keep hardware I/O thin. The logic that matters is testable without hardware and tested through the public API in
  `voice/tests/`: plan parsing, progress counting from the manifest, the picker, removals, and the push-to-talk
  segmenter (feed it sample blocks and button events, check the produced take boundaries including pre/post roll).
- House style from CLAUDE.md applies (explicit `return`, test first, few comments, etc.).

## Files and data

In git:

- `voice/plan.toml`: the checklist definition: prompts per kind, conditions (id + label), styles, targets per kind,
  near-miss list. Hand-edited; the app never writes to it.

Not in git (`voice-data/` at the repo root, added to `.gitignore`; path overridable by a command-line flag):

- `voice-data/takes/<kind>/<prompt-id>/<condition-id>/<speaker>__<UTC timestamp>__<style>.wav`
  Prompt ids are filesystem-safe slugs (`turn-left`, `hommel-flip`).
- `voice-data/manifest.csv`: append-only, one row per event. Columns:
  `time_utc, session_id, event (take|rejected), file, kind, prompt_id, prompt_text, condition_id, style, speaker,
  duration_ms, sample_rate, channels, device_name, capture_volume, peak, warnings`.
  A redo appends a `rejected` row for the file and moves the WAV to `voice-data/rejected/` (kept, not deleted; might be
  useful as noise later). Progress = `take` rows minus `rejected` ones.
- `voice-data/removed.csv`: one row per removed cell (`prompt_id, condition_id, time_utc`). Delete a row to undo.
  (Earlier in the conversation this was described as "marked in the plan file"; it lives here instead so the app never
  rewrites the hand-edited, git-tracked plan.)
- Each WAV and its manifest row are written (and the manifest flushed) before the next prompt shows, so a crash loses
  at most the take being recorded.

## What the training side can rely on

- Every take's WAV, its labels (kind, prompt, condition, style, speaker) and session id are in the manifest.
- Sessions are recorded as units, so training can split train/test by session or condition rather than by random clip
  (random-clip splits overstate accuracy).
- Audio is untouched 48 kHz from the in-flight mic path.

## Left to the builder

- Exact egui layout, fonts, colours (should follow the mockup: big prompt, clear button hints).
- How the level meter and waveform are drawn.
- Internal module structure of the two crates.
- Error messages and their wording.
