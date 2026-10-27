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

Below, a client and a drone talk over a byte stream: anything with `Read + Write`, such as the serial port to the
radio module or a TCP stream. Each message and event is a plain struct, and the types decide what can be said, so
there are no status bytes or reason numbers to get wrong. Over UDP one datagram is one packet: skip
`StreamDecoder` and `stream::encode`, read with `Packet::parse(&datagram)` and send `packet.as_bytes()`.

### Both sides: sending

```rust
use drone_common::wire::message::Message;
use drone_common::wire::packet::MAX_PACKET_LEN;
use drone_common::wire::stream::{self, MAX_FRAME_LEN};
use std::io::Write;

pub const PROTOCOL_VERSION: u8 = 1;

/// Sends one message as a frame. Every message sent counts up the sequence number (protocol.md §4.1).
pub fn send(link: &mut impl Write, sequence: &mut u16, message: Message) -> std::io::Result<()> {
    let mut packet = [0u8; MAX_PACKET_LEN];
    let mut frame = [0u8; MAX_FRAME_LEN];
    link.write_all(stream::encode(&message.encode(*sequence, &mut packet), &mut frame))?;
    *sequence = sequence.wrapping_add(1);
    return Ok(());
}
```

### Client

Opens a session, asks for a flip, then handles whatever comes back. Not shown: acknowledging what arrives and
resending what gets lost (protocol.md §5.0a).

```rust
use drone_common::wire::event::{Event, EventKind};
use drone_common::wire::message::{Answer, Command, CommandMessage, Hello, HelloOptions, HelloResult, Message};
use drone_common::wire::message::RequestId;
use drone_common::wire::stream::StreamDecoder;
use std::error::Error;
use std::io::{Read, Write};

fn client(link: &mut (impl Read + Write)) -> Result<(), Box<dyn Error>> {
    let mut sequence = 0; // Hello is message 0 of a session
    let hello = Hello {
        protocol_version: PROTOCOL_VERSION,
        options: HelloOptions::default(),
    };
    send(link, &mut sequence, Message::Hello(hello))?;

    // A real client sends this after the HelloReply, with Sticks going out at 50 Hz all the time (§5.2).
    // Requests count up their own Request ID; a resend keeps it (protocol.md §5.0a). Sent At is the client's
    // session clock in µs: the drone refuses a Flip older than COMMAND_EXPIRY_US.
    let flip = Command::Flip {
        direction: 0xC000, // to the left
        duration_ms: 300,
    };
    send(link, &mut sequence, Message::Command(CommandMessage::new(RequestId(1), 0, flip)))?;

    let mut decoder = StreamDecoder::new();
    let mut read_buf = [0u8; 512];
    loop {
        let n = link.read(&mut read_buf)?;
        let mut chunk = &read_buf[..n];
        while let Some(packet) = decoder.next_packet(&mut chunk) {
            let Ok(packet) = packet else { continue }; // damaged on the way: drop it, count it as lost
            match Message::decode(&packet) {
                Ok(Message::HelloReply(reply)) => match reply.result {
                    HelloResult::Ok => println!("connected"),
                    refused => return Err(format!("drone refused the session: {refused:?}").into()),
                },
                Ok(Message::Reply(reply)) => match reply.answer() {
                    Answer::Flip(outcome) => println!("flip {:?}: {outcome:?}", reply.request_id()),
                    answer => println!("{:?}: {answer:?}", reply.request_id()),
                },
                // A resent event keeps its Event ID: a real client shows each ID once (protocol.md §5.0a).
                Ok(Message::Event(Event { kind: EventKind::Text(text), .. })) => {
                    println!("{:?}: {}", text.level, text.text);
                }
                Ok(Message::Event(Event { kind, .. })) => println!("{:?}: {kind:?}", kind.level()),
                Ok(other) => println!("ignored {other:?}"),
                Err(error) => println!("dropped an invalid message: {error}"), // only the drone answers these
            }
        }
    }
}
```

### Drone

Answers every request: with its outcome, or with "malformed" / "unknown type" when it doesn't decode. Deciding
*whether* to arm or flip is the flight code's job; the types only let it answer with reasons that request may have.
Not shown: acknowledging and resending (protocol.md §5.0a), remembering the last 16 Request IDs so a resent
request is not executed twice, and refusing an old TakeOff or Flip with `command.refuse_too_late()` (§5.3).

```rust
use drone_common::wire::message::{ArmRefusal, CommandMessage, HelloOptions, HelloReply, HelloResult, Message, Options};
use drone_common::wire::reply::{Outcome, Reply};
use drone_common::wire::stream::StreamDecoder;
use std::error::Error;
use std::io::{Read, Write};

/// `flight` is your flight code: it takes the sticks and decides each request, returning its Outcome (for Disarm an
/// `Outcome<Infallible>`, which can't be a refusal).
fn drone(link: &mut (impl Read + Write), flight: &mut Flight) -> Result<(), Box<dyn Error>> {
    let mut sequence = 0;
    let mut decoder = StreamDecoder::new();
    let mut read_buf = [0u8; 512];
    loop {
        let n = link.read(&mut read_buf)?;
        let mut chunk = &read_buf[..n];
        while let Some(packet) = decoder.next_packet(&mut chunk) {
            let Ok(packet) = packet else { continue }; // damaged on the way: drop it, count it as lost
            let answer = match Message::decode(&packet) {
                Ok(Message::Hello(hello)) => {
                    sequence = 0; // a Hello starts a new session; the HelloReply is its message 0
                    let result = if hello.protocol_version != PROTOCOL_VERSION {
                        HelloResult::VersionMismatch
                    } else if hello.options != HelloOptions::default() {
                        HelloResult::OptionNotSupported // e.g. authentication, not built yet
                    } else {
                        HelloResult::Ok
                    };
                    Some(Message::HelloReply(HelloReply {
                        protocol_version: PROTOCOL_VERSION,
                        result,
                        options: Options::default(),
                    }))
                }
                Ok(Message::Sticks(sticks)) => {
                    flight.set_sticks(sticks);
                    None // Sticks are never answered
                }
                // A Reply can only be built from the request it answers, with the outcomes that request may have.
                Ok(Message::Command(command)) => Some(Message::Reply(match command {
                    CommandMessage::Arm(arm) => {
                        let outcome = if flight.arm_switch_on() {
                            flight.arm()
                        } else {
                            Outcome::Refused(ArmRefusal::ArmSwitchOff) // the transmitter has the last word
                        };
                        Reply::to(&arm, outcome)
                    }
                    CommandMessage::Disarm(disarm) => Reply::to(&disarm, flight.disarm()), // never refused
                    CommandMessage::Flip(flip) => Reply::to(&flip, flight.flip(&flip)),
                    other => other.reply(Outcome::NotSupported), // an outcome every command may have
                })),
                Ok(other) => {
                    println!("not handled yet: {other:?}");
                    None
                }
                Err(_) => Reply::to_invalid(&packet).map(Message::Reply), // "malformed" / "unknown type"
            };
            if let Some(answer) = answer {
                send(link, &mut sequence, answer)?;
            }
        }
        // The drone also reports on its own, e.g. when the battery runs low:
        // `event_id` counts up with `wrapping_add` for every new event:
        // let kind = EventKind::BatteryLow(BatteryLow { millivolts });
        // send(link, &mut sequence, Message::Event(Event { event_id, kind }))?;
    }
}
```

## Modules

| Module | Public items | What it is |
|--------|--------------|------------|
| `wire::message` | `Message`, one struct per message (`Hello`, `Ping`, `Ack`, `Arm`, `Flip`, …), `Command`, `CommandMessage`, `FlightMode`, `HelloResult`, `Options` (known options), `HelloOptions` (also the `UnknownOptions` a Hello may carry), the refusal codes (`ArmRefusal`, …), `Request`, `RequestId`, `Answer`, `RequestType`, `UnknownType`, `MessageError` | The messages of protocol.md §5 and their `encode` / `decode`. Each message is defined once, in one list |
| `wire::reply` | `Reply`, `Outcome`, `Reason` | The drone's answer to a request, typed per request: only combinations the protocol allows can be built, and only as the answer to a request. Every refusal enum (`FlipRefusal`, …) converts into the shared `Reason`, so the client can handle any refusal in one place. `Reply::to(&request, outcome)` takes the Request ID and type from the request; `Reply::to_invalid(&packet)` answers a packet that didn't decode |
| `wire::event` | `Event` (the message), `EventId`, `EventKind`, one struct per event (`Armed`, `BatteryLow { millivolts }`, …), `EventLevel`, `FailsafeCause`, `FlightModeChanged`, `Text`, `EventText`, `FixedEvent` | What the drone reports on its own; each event holds only its own data. `EventText` is a string without heap: use it like a `&str`, build it with `EventText::format(format_args!(…))` |
| `wire::packet` | `Packet`, `PacketError`, `MAX_PACKET_LEN` | A checked packet: `parse`, `sequence`, `as_bytes` |
| `wire::stream` | `encode`, `StreamDecoder`, `MAX_FRAME_LEN` | COBS framing; only needed on byte streams |
| `domain` | `Sticks` (the Sticks message, re-exported), `Command` (re-exported: a command without the Request ID and Sent At of its message; `CommandMessage::new(..)` and `command_message.command()` convert), `ArmState`, `ArmSwitch`, `FlightMode` (re-exported), `LinkHealth`, `Uptime`, `RcChannels` | Protocol-neutral state: what the sticks say, what the pilot asks, armed or not, the arm switch, what the sticks mean, when the client was last heard |
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
