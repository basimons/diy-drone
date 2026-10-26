# drone-common

The language the ground client and the flight controller (FC) both speak: types and bytes, no behaviour.
It is `no_std` (needs no operating system) and uses no heap, so the flight loop can use it safely.
The protocol itself is described in [../protocol.md](../protocol.md); §7 there explains how the code is split up.

## What you use

```rust
use drone_common::wire::message::{MAX_ENCODED_LEN, Message, Ping};
use drone_common::wire::splitter::FrameSplitter;

// Sending: a message plus your sequence number in, ready-to-send bytes out.
let mut buf = [0u8; MAX_ENCODED_LEN];
let len = Message::Ping(Ping { timestamp: 1234 }).encode(sequence, &mut buf);
port.write_all(&buf[..len])?;

// Receiving from a byte stream (serial, radio): reads arrive in chunks of any size,
// so the splitter cuts them into frames, then each frame is decoded.
let mut splitter = FrameSplitter::new();
let n = port.read(&mut read_buf)?;
let mut chunk = &read_buf[..n];
while let Some(frame) = splitter.next_frame(&mut chunk) {
    match frame.and_then(Message::decode) {
        Ok(received) => handle(received.sequence, received.message),
        Err(error) => stats.count(error), // dropped, e.g. DecodeError::BadCrc
    }
}

// Receiving over UDP: one packet is one frame, so no splitter.
let received = Message::decode(&packet)?;
```

| Module | Public items | What it is |
|--------|--------------|------------|
| `wire::message` | `Message`, one struct per message (`Hello`, `HelloReply`, `Ping`, `Pong`, …), `Received`, `DecodeError`, `MAX_ENCODED_LEN` | The messages of protocol.md §5 and their `encode` / `decode` |
| `wire::splitter` | `FrameSplitter` | Cuts a byte stream into frames; only needed on stream transports |
| `wire::codes` | `MessageType`, `HelloResult`, … | The numeric codes used inside messages |
| `domain` | `RcChannels`, `ArmState`, … | Protocol-neutral state: what the sticks say, armed or not |
| `crsf` | CRSF constants and CRC-8 | CRSF (Crossfire), the radio protocol between the ELRS receiver and the FC |

## What is hidden

Every message travels inside an *envelope* (flags, type, sequence number, payload, CRC-16 checksum, protocol.md §4.1).
The envelope is then COBS-encoded (Consistent Overhead Byte Stuffing, §4.2): all `0x00` bytes are removed so that a
single `0x00` can mark the end of each frame. Both steps happen inside `Message::encode` and `Message::decode`, so
the envelope, CRC and COBS code are private. The splitter only knows that frames end at `0x00`.

## What is not in here

- **Sequence counters.** The caller keeps its own outgoing counter (use `wrapping_add`) and decides what to do
  with incoming sequence numbers (gaps, stale messages). Helpers for that arithmetic would go in a separate
  `drone-link` crate, which doesn't exist yet.
- **Decisions and I/O.** Whether to obey a request, timers, serial ports and sockets live in `fc` and `client`.

## Tests

Tests are in `tests/` and use only the public API. `tests/support/` builds frames by hand, with its own CRC-16 and
COBS code, so the library is checked against an independent implementation. `tests/encoding.rs` also pins exact
frame bytes computed outside Rust.

```sh
cargo test -p drone-common
```
