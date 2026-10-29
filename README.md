# DIY Pi Drone

A 5" quadcopter where a Raspberry Pi 5 runs hand-written flight code in Rust.
Hardware, wiring and goals are in [drone-design.md](drone-design.md). The client ↔ drone link protocol is in [protocol.md](protocol.md).

## Projects

| Folder | Crate | Runs on | What it does |
|--------|-------|---------|--------------|
| `common/` | `drone-common` | anywhere (no OS needed) | Shared domain types, our link protocol's messages and framing, and the CRSF byte protocol ([common/README.md](common/README.md)) |
| `client/` | `drone-client` | Windows, macOS, Linux | Reads the gamepad, encodes CRSF frames, sends them over USB serial to the ELRS transmitter module |
| `fc/` | `drone-fc` | Linux (Raspberry Pi 5, or any board with SPI/I2C/GPIO) | Flight code: sensors, stabilization, motors, protections |
| `voice/` | `drone-voice` | Windows, macOS, Linux | Voice commands on the ground: the recording checklist, takes on disk, push-to-talk capture from the PS5 controller's mic ([voice/README.md](voice/README.md)) |
| `voice-recorder/` | `voice-recorder` | Windows, macOS, Linux | A small window that walks you through recording the voice-command training data |

`client` and `fc` both depend on `common`, so the two sides of the link always agree on the format.

## Getting started

Install Rust with [rustup](https://rustup.rs), then from this folder:

```sh
cargo build                  # build everything
cargo test                   # run the tests
cargo run -p drone-client    # run the ground client
cargo run -p drone-fc        # run the flight code (on the laptop it only prints some startup info)
scripts/check.sh             # formatting, lints and tests: the same checks CI runs
```

Once per clone, let git run `scripts/check.sh` before every commit:

```sh
git config core.hooksPath .githooks
```

Pull requests run the same checks on GitHub Actions (`.github/workflows/ci.yml`).

On Linux the client needs `libudev` (preinstalled on most desktops; `libudev-dev` / `systemd-devel` to build).
The voice crates also need ALSA, the Linux sound system (`libasound2-dev` / `alsa-lib-devel` to build).

## Recording voice commands

The drone will also take spoken commands ("Hommel, flip"). Training the speech model needs recordings, made with
the same microphone used in flight: the one in the PS5 controller, plugged in over USB (over Bluetooth Linux gets
no microphone). To record:

```sh
cargo run -p voice-recorder                        # data goes to voice-data/ (not in git)
cargo run -p voice-recorder -- --data /some/dir    # somewhere else
```

At start-up, tick where you are right now (quiet room, outdoors, …) and what to record; the app only asks for
what is still missing there. Hold **R1** while you say the word on screen. **○** redoes the last take, **△** plays it
back, **□** skips a word for this session (hold it 1 s to never ask again), **OPTIONS** pauses or ends. Everything
is saved after each take, so you can stop any time and carry on later. The checklist itself is
[voice/plan.toml](voice/plan.toml); [voice/README.md](voice/README.md) explains the files it writes.

## Building for the Pi

The Pi 5 is 64-bit ARM, so build on the laptop for that target:

```sh
rustup target add aarch64-unknown-linux-gnu
cargo build -p drone-fc --release --target aarch64-unknown-linux-gnu
```

This needs an ARM linker (`gcc-aarch64-linux-gnu`). Alternatively, use
[`cross`](https://github.com/cross-rs/cross) (needs Docker) with the same arguments.
Then copy `target/aarch64-unknown-linux-gnu/release/drone-fc` to the Pi.

## Status

- [x] Workspace skeleton, hello world in all three projects
- [x] CRSF checksum (CRC-8/DVB-S2) with test
- [ ] CRSF RC channel frame encode/decode
- [ ] Client: gamepad → channels → serial
- [ ] FC: read CRSF from the receiver UART
- [ ] FC: IMU over SPI
- [ ] FC: motor output, protections
- [x] Voice: recorder for the training data (`voice-recorder`)
- [ ] Voice: train the model (Python), live recognition in the client
