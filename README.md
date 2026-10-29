# DIY Pi Drone

A 5" quadcopter where a Raspberry Pi 5 runs hand-written flight code in Rust.
Hardware, wiring and goals are in [drone-design.md](drone-design.md). The client ↔ drone link protocol is in [protocol.md](protocol.md).

## Projects

| Folder | Crate | Runs on | What it does |
|--------|-------|---------|--------------|
| `common/` | `drone-common` | anywhere (no OS needed) | Shared domain types, our link protocol's messages and framing, and the CRSF byte protocol ([common/README.md](common/README.md)) |
| `client/` | `drone-client` | Windows, macOS, Linux | Reads the gamepad, encodes CRSF frames, sends them over USB serial to the ELRS transmitter module |
| `fc/` | `drone-fc` | Linux (Raspberry Pi 5, or any board with SPI/I2C/GPIO) | Flight code: sensors, stabilization, motors, protections |

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
