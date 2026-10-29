# drone-voice

Voice commands on the ground side. Today: everything `voice-recorder` needs to collect training data. Later the client
uses the same microphone code for live recognition, so the model hears in flight exactly what it was trained on.
Design: [docs/superpowers/specs/2026-10-29-voice-recorder-design.md](../docs/superpowers/specs/2026-10-29-voice-recorder-design.md).

Two ways of talking to the drone are planned:

- **Push-to-talk**: hold R1 on the PS5 controller and say only the command.
- **Wake word**: the ground station listens all the time for the drone's name, **Hommel** (Dutch for bumblebee), and
  treats the next ~2 seconds as a command ("Hommel, flip").

## What the crate offers

| Item | What it does |
|------|--------------|
| `Plan` | The checklist from [plan.toml](plan.toml): prompts, conditions, speaking styles, targets |
| `Store` | Saves takes (WAV + manifest row), rejects and removes them, counts progress |
| `pick` | Chooses the prompts for one session from what is still missing |
| `PushToTalk`, `Chunker` | Cut the mic stream into takes (300 ms before R1 until 300 ms after) or 60 s background pieces |
| `warnings` | Flags takes that are clipped, very quiet, too short or too long |
| `Mic` | The DualSense microphone: 48 kHz, mono, 16-bit, as one continuous stream |

The microphone test needs the controller on USB: `cargo test -p drone-voice --test mic_hardware -- --ignored`.

## The plan file

Every **prompt × condition** pair is one checklist item (a "cell") that needs `target` takes. Conditions are where
you are when recording (quiet room, outdoors windy, …). Phrases ("Hommel, flip") are generated from the wake word and
the commands, so they are never listed twice. Raise a target to put cells back on the checklist; the app never
writes this file.

Noise that can be added afterwards (TV, music, a fan) is only recorded as **background**: long hands-free
recordings that training mixes into clean takes ("augmentation"). Takes themselves are only spoken in conditions that
can't be faked well.

## Data on disk (the contract for training)

Everything lives under the data folder (default `voice-data/`, not in git):

```
voice-data/
  manifest.csv          one row per take or rejection, append-only
  removed.csv           cells the user asked never to see again (delete a row to undo)
  takes/<kind>/<prompt-id>/<condition-id>/<speaker>__<UTC time>__<style>.wav
  rejected/takes/…      takes that were redone, same paths; kept, maybe useful as noise
```

- `<kind>` is `command`, `wake-word`, `phrase`, `near-miss` or `background` (background uses prompt id
  `background` and style `none`).
- WAVs are **48 kHz, mono, 16-bit**, exactly as the controller recorded them: no gain, filtering or resampling.
  Downsample (for example to 16 kHz) at training time.

`manifest.csv` columns:

| Column | Meaning |
|--------|---------|
| `time_utc` | when the take was saved (or rejected) |
| `session_id` | UTC start of the recording session; split train/test **by session or condition**, not by random clip |
| `event` | `take`, or `rejected` for a take that was redone (it no longer counts) |
| `file` | WAV path relative to the data folder, always with `/` |
| `kind` | `command`, `wake_word`, `phrase`, `near_miss` or `background` |
| `prompt_id`, `prompt_text` | e.g. `turn-left`, `turn left` |
| `condition_id` | e.g. `quiet-room` |
| `style` | how it was asked to be said (`fast`, `annoyed`, …); empty for background |
| `speaker` | name typed at start-up |
| `duration_ms`, `sample_rate`, `channels`, `device_name` | about the audio and the device (`channels` is what the device delivers; files are mono) |
| `capture_volume` | the controller's mic level in the OS mixer, e.g. `31%`, if readable |
| `peak` | loudest sample, 0–32768 |
| `warnings` | `;`-separated: `clipped`, `very_quiet`, `too_short`, `too_long` |

A take counts when it has a `take` row and no `rejected` row for the same `file`.
