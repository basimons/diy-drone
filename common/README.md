# drone-common

The language the ground client and the flight controller (FC) both speak: types and bytes, no behaviour.
It is `no_std` (needs no operating system) and uses no heap, so the flight loop can use it safely.
The protocol itself is described in [../protocol.md](../protocol.md); §7 there explains how the code is split up.

## What you use

```rust
use drone_common::wire::frame::{FrameReader, MAX_FRAME_LEN, Push};
use drone_common::wire::message::{Message, Ping};

// Sending: a message plus your sequence number in, ready-to-send bytes out.
let mut buf = [0u8; MAX_FRAME_LEN];
let len = Message::Ping(Ping { timestamp: 1234 }).encode_frame(sequence, &mut buf);
port.write_all(&buf[..len])?;

// Receiving: feed bytes in as they arrive, one at a time.
let mut reader = FrameReader::new();
for &byte in &received_bytes {
    match reader.push(byte) {
        Push::NeedMore => {}                               // no complete frame yet
        Push::Frame(received) => handle(received.sequence, received.message),
        Push::Rejected(error) => stats.count(error),       // dropped frame, e.g. FrameError::BadCrc
    }
}
```

| Module | Public items | What it is |
|--------|--------------|------------|
| `wire::message` | `Message` and one struct per message (`Hello`, `HelloReply`, `Ping`, `Pong`, …) | The messages of protocol.md §5, as typed Rust values |
| `wire::frame` | `FrameReader`, `Push`, `Received`, `FrameError`, `MAX_FRAME_LEN` | Turning a byte stream into messages |
| `wire::codes` | `MessageType`, `HelloResult`, … | The numeric codes used inside messages |
| `domain` | `RcChannels`, `ArmState`, … | Protocol-neutral state: what the sticks say, armed or not |
| `protocol` | CRSF constants and CRC-8 | CRSF (Crossfire), the radio protocol between the ELRS receiver and the FC |

## What is hidden

Every message travels inside an *envelope* (flags, type, sequence number, payload, CRC-16 checksum, protocol.md §4.1).
The envelope is then COBS-encoded (Consistent Overhead Byte Stuffing, §4.2): all `0x00` bytes are removed so that a
single `0x00` can mark the end of each frame. Both steps happen inside `encode_frame` and `FrameReader`, so the
envelope, CRC and COBS code are private.

## What is not in here

- **Sequence counters.** The caller keeps its own outgoing counter (use `wrapping_add`) and decides what to do
  with incoming sequence numbers (gaps, stale messages). Helpers for that arithmetic would go in a separate
  `drone-link` crate, which doesn't exist yet.
- **Decisions and I/O.** Whether to obey a request, timers, serial ports and sockets live in `fc` and `client`.

## Tests

Tests are in `tests/` and use only the public API. `tests/support/` builds frames by hand, with its own CRC-16 and
COBS code, so the library is checked against an independent implementation. `tests/frame.rs` also pins exact
frame bytes computed outside Rust.

```sh
cargo test -p drone-common
```
