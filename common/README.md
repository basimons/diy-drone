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
use drone_common::wire::reply::Reply;
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
        Err(error) => reject(error, Reply::to_invalid(&packet)), // "malformed" / "unknown type", or None
    }
}
```

## Messages, by example

One request from start to finish, plus some events. Each message and event is a plain struct; the types decide what
can be said, so there are no status bytes or reason numbers to get wrong.

```rust
use drone_common::wire::event::{BatteryLow, Event, EventLevel, EventText, Text};
use drone_common::wire::message::{Answer, Flip, FlipRefusal, Message, RequestId};
use drone_common::wire::reply::{Failure, Reply};

// Client: ask for a flip. Every request starts with a Request ID; a resend keeps the same ID.
let flip = Flip {
    request_id: RequestId(7),
    direction: 0xC000, // to the left
    duration_ms: 300,
};
let received = Message::Flip(flip);

// Drone: decide, then answer. `Reply::to` copies the Request ID and type from the request, and a Flip only
// accepts a `FlipRefusal`: `Failure::Refused(ArmRefusal::BatteryLow)` doesn't compile.
let reply = match received {
    Message::Flip(flip) => Reply::to(&flip, Err(Failure::Refused(FlipRefusal::TooLow))),
    Message::Arm(arm) => Reply::to(&arm, Ok(())),
    // Disarm is never refused: `Failure::Refused` can't be built for it, `Failure::Busy` can.
    Message::Disarm(disarm) => Reply::to(&disarm, Err(Failure::Busy)),
    _ => return,
};

// Drone: report on its own. Each event type has a fixed code and level; Text can have any level.
let battery = Event::BatteryLow(BatteryLow { millivolts: 14_000 });
let text = EventText::format(format_args!("cell {} at {:.1} V", 3, 3.35)).unwrap(); // None if empty or > 246 bytes
let note = Event::Text(Text { level: EventLevel::Info, text });

// Client: handle what comes back.
for message in [Message::Reply(reply), Message::Event(battery), Message::Event(note)] {
    match message {
        Message::Reply(Reply { request_id, answer: Answer::Flip(Ok(())) }) => println!("{request_id:?}: flipping"),
        Message::Reply(Reply { request_id, answer: Answer::Flip(Err(failure)) }) => {
            println!("{request_id:?}: no flip, {failure:?}") // RequestId(7): no flip, Refused(TooLow)
        }
        Message::Event(Event::Text(Text { level, text })) => println!("{level:?}: {text}"), // Info: cell 3 at 3.4 V
        Message::Event(event) => println!("{:?}: {event:?}", event.level()), // Warning: BatteryLow(BatteryLow { millivolts: 14000 })
        _ => {}
    }
}
```

A packet that doesn't decode is answered with `Reply::to_invalid(&packet)`: "malformed" for a request, "unknown type"
for a type this build doesn't know (protocol.md §5.4).

## Modules

| Module | Public items | What it is |
|--------|--------------|------------|
| `wire::message` | `Message`, one struct per message (`Hello`, `Ping`, `Arm`, `Flip`, …), `HelloResult`, the refusal codes (`ArmRefusal`, …), `Request`, `RequestId`, `Answer`, `RequestType`, `UnknownType`, `MessageError` | The messages of protocol.md §5 and their `encode` / `decode`. Each message is defined once, in one list |
| `wire::reply` | `Reply`, `Failure` | The drone's answer to a request, typed per request: only combinations the protocol allows can be built. `Reply::to(&request, outcome)` takes the Request ID and type from the request; `Reply::to_invalid(&packet)` answers a packet that didn't decode |
| `wire::event` | `Event`, one struct per event (`Armed`, `BatteryLow { millivolts }`, …), `EventLevel`, `FailsafeCause`, `Text`, `EventText`, `EventKind` | What the drone reports on its own; each event holds only its own data. `EventText` is a string without heap: use it like a `&str`, build it with `EventText::format(format_args!(…))` |
| `wire::packet` | `Packet`, `PacketError`, `MAX_PACKET_LEN` | A checked packet: `parse`, `sequence`, `as_bytes` |
| `wire::stream` | `encode`, `StreamDecoder`, `MAX_FRAME_LEN` | COBS framing; only needed on byte streams |
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
