# drone-common

The language the ground client and the flight controller (FC) both speak: types and bytes, no behaviour.
It is `no_std` (needs no operating system) and uses no heap, so the flight loop can use it safely.
The protocol itself is described in [../protocol.md](../protocol.md); §7 there explains how the code is split up.

## What you use

Three layers, each with its own type:

```
Message  ⇄  Packet  ⇄  frame (byte streams only)
```

- **`Message`**: a typed message, e.g. `Message::Ping(Ping { timestamp })`.
- **`Packet`**: one message as bytes: flags, type, sequence number, payload, CRC-16 checksum (protocol.md §4.1). A
  `Packet` can only exist after its length, checksum and flags were checked, so `Message::decode` never sees
  unchecked bytes. Over WiFi/UDP one datagram is one packet.
- **Frame**: on a byte stream (serial, radio), a packet is COBS-encoded (Consistent Overhead Byte Stuffing): all
  `0x00` bytes are replaced by signposts, and one `0x00` ends the frame, so the receiver knows where each packet
  ends (protocol.md §4.2).

```rust
use drone_common::wire::message::{Message, Ping};
use drone_common::wire::packet::{MAX_PACKET_LEN, Packet};
use drone_common::wire::stream::{self, MAX_FRAME_LEN, StreamDecoder};

// Illustrative: `socket`, `port`, `handle`, … are yours. Both error types implement `core::error::Error`,
// so `?` works with e.g. anyhow.
let ping = Message::Ping(Ping { timestamp: 1234 });
let mut buf = [0u8; MAX_PACKET_LEN];

// UDP
// Hello and HelloReply are message 0 of a session: encode them with sequence 0.
socket.send(ping.encode(sequence, &mut buf).as_bytes())?;
let packet = Packet::parse(&datagram)?;            // PacketError: damaged, drop and count it
let received = Message::decode(&packet)?;          // MessageError: intact but invalid, can be answered

// Byte stream (serial, radio): reads arrive in chunks of any size
let mut frame = [0u8; MAX_FRAME_LEN];
port.write_all(stream::encode(&ping.encode(sequence, &mut buf), &mut frame))?;

let mut decoder = StreamDecoder::new();
let mut chunk = &read_buf[..n];
while let Some(result) = decoder.next_packet(&mut chunk) {
    let Ok(packet) = result else { continue };     // count the PacketError
    match Message::decode(&packet) {
        Ok(message) => handle(packet.sequence(), message),
        Err(error) => answer(&packet, error),      // the packet is still there for the Reply
    }
}
```

| Module | Public items | What it is |
|--------|--------------|------------|
| `wire::message` | `Message`, one struct per message (`Hello`, `Ping`, `Arm`, `Flip`, …), `Request`, `RequestId`, `Answer`, `UnknownType`, `MessageError` | The messages of protocol.md §5 and their `encode` / `decode` |
| `wire::reply` | `Reply`, `Failure` | The drone's answer to a request, typed per request: only combinations the protocol allows can be built. `Reply::to(&request, outcome)` takes the Request ID and type from the request |
| `wire::event` | `Event`, one struct per event (`Armed`, `BatteryLow { millivolts }`, …), `Text`, `EventKind` | What the drone reports on its own; each event holds only its own data |
| `wire::packet` | `Packet`, `PacketError`, `MAX_PACKET_LEN` | A checked packet: `parse`, `sequence`, `as_bytes` |
| `wire::stream` | `encode`, `StreamDecoder`, `MAX_FRAME_LEN` | COBS framing; only needed on byte streams |
| `wire::codes` | `HelloResult`, `EventLevel`, `FailsafeCause`, `ArmRefusal` (one refusal enum per request), … | The numeric codes used inside messages. Each holds only the protocol's values, so whatever you build can be sent; an unknown number fails decoding with `MessageError::UnknownCode` |
| `domain` | `Sticks`, `ArmState`, `RcChannels` | Protocol-neutral state: what the sticks say, armed or not |
| `crsf` | CRSF constants and CRC-8 | CRSF (Crossfire), the radio protocol between the ELRS receiver and the FC |

The CRC-16 and COBS code itself is private.

## What is not in here

- **Sequence counters.** The caller keeps its own outgoing counter (use `wrapping_add`) and decides what to do
  with incoming sequence numbers (gaps, stale messages). Helpers for that arithmetic would go in a separate
  `drone-link` crate, which doesn't exist yet.
- **Decisions and I/O.** Whether to obey a request, timers, serial ports and sockets live in `fc` and `client`.

## Tests

Tests are in `tests/` and use only the public API. `tests/support/` builds packets and frames by hand, with its own
CRC-16 and COBS code, so the library is checked against an independent implementation. `tests/packet.rs` and
`tests/stream.rs` also pin exact bytes computed outside Rust.

```sh
cargo test -p drone-common
```
