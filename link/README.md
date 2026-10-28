# drone-link

The link rules both the ground client and the flight controller follow: acknowledging what arrives, finding out
what got lost so it can be resent, measuring the round-trip time, and recognising a resend so it isn't acted on
twice (protocol.md §5.0 and §5.0a). Like `drone-common` it is `no_std` and uses no heap.

It says what is true about the link and leaves the decisions to the caller: "this message is lost" (resend it, or drop
it if it expired?), "this Request ID is a repeat" (acknowledge, but don't flip twice). It does no I/O and reads no
clock: you pass in the time, as a `SessionTime` (µs on the session clock, protocol.md §5.1).

## Pieces

Each one works on its own; `fc` and `client` combine them.

| Module | Type | What it does |
|--------|------|--------------|
| `outbox` | `Outbox<T, N>` | Sending side. Holds up to `N` reliable messages until they are acknowledged. From an Ack (`on_ack`) or the probe timeout (`check_timeouts`) it reports each one as `Delivery::Acknowledged` or `Delivery::Lost`, in send order. `acknowledge` takes out a request whose answer arrived; `discard` drops a message the caller gives up on (the drone's rules in protocol.md §5.0a); `items` shows what is waiting `T` is whatever you want back: the message, or an index into your own list |
| `ack` | `AckTracker` | Receiving side. Records which Sequences arrived (feed it from the session's first message, the Hello or HelloReply), says when an Ack is owed (`ack_due`, at most `MAX_ACK_DELAY` = 20 ms after a reliable message) and builds it (`ack`). Each arrival is `Newest { skipped }`, `Late`, `Duplicate` or `TooOld`, for counting gaps |
| `rtt` | `RttEstimator` | Round-trip time (RTT): smoothed RTT and its variation as in QUIC (RFC 9002), and the probe timeout that follows. The `Outbox` keeps one; start it from a Ping/Pong if you have one |
| `recent_ids` | `RecentIds<Id, N>` | "Have I handled this ID among the last `N`?" The drone uses it for Request IDs (`DRONE_ID_WINDOW` = 32), the client for Event IDs and answered Request IDs (`CLIENT_ID_WINDOW` = 1024; it may use any list it likes, also on the heap) |
| `time` | `SessionTime` | A moment on the session clock; `since` uses wrapping arithmetic |

Which messages are reliable is a fact about the message: `Message::is_reliable` in `drone-common`.

## Using it

One side of the link, put together. Sending and resending:

```rust
use drone_common::wire::message::{DRONE_ID_WINDOW, Message, RequestId};
use drone_link::ack::AckTracker;
use drone_link::outbox::{Delivery, Outbox};
use drone_link::recent_ids::{RecentIds, Seen};
use drone_link::rtt::RttEstimator;
use drone_link::time::SessionTime;

struct Side {
    next_sequence: u16,
    acks: AckTracker,
    outbox: Outbox<Message, DRONE_ID_WINDOW>,       // the client: requests waiting stay that close (protocol.md §5.0a)
    handled: RecentIds<RequestId, DRONE_ID_WINDOW>, // the drone: the requests it executed
}

impl Side {
    /// `Err(message)` while the outbox is full: wait for Acks, or drop something less important.
    #[allow(clippy::result_large_err)] // owned messages are ~250 bytes (protocol.md §7.1)
    fn send(&mut self, message: Message, now: SessionTime) -> Result<(), Message> {
        if message.is_reliable() && self.outbox.is_full() {
            return Err(message); // before taking a Sequence, so none is wasted
        }
        let sequence = self.next_sequence;
        self.next_sequence = sequence.wrapping_add(1);
        if message.is_reliable() {
            let _ = self.outbox.send(sequence, message, now); // can't be full: checked above
        }
        // ... encode with `sequence` and write it to the link
        return Ok(());
    }
```

Receiving (the drone's side of a command):

```rust
    fn receive(&mut self, sequence: u16, message: Message, now: SessionTime) {
        self.acks.on_receive(sequence, message.is_reliable(), now);
        match message {
            Message::Ack(ack) => self.outbox.on_ack(&ack, now, |delivery| {
                if let Delivery::Lost(lost) = delivery {
                    // resend `lost`: `send` it again, with a new Sequence
                }
            }),
            // The client, on a Reply: `self.outbox.acknowledge(..)` the request it answers.
            Message::Command(command) => match self.handled.remember(command.request_id()) {
                Seen::New => { /* execute it and send the Reply */ }
                Seen::Repeat => {} // a resend: the Ack above covers it, don't execute twice
            },
            _ => {}
        }
    }
```

And on a timer: send the Ack when `acks.ack_due(now)` reaches zero, and call `outbox.check_timeouts(now, …)` when
`outbox.next_timeout(now)` does, resending what it reports lost.

The snippets leave out the collecting of lost messages: `on_ack` borrows the outbox, so gather them in a list
and send them after it returns. [tests/session.rs](tests/session.rs) has the whole thing, compiled and run. It plays
the example of protocol.md §5.0: the Reply and the Ack are both lost, the client resends, the drone arms only once,
and both outboxes end up empty.

[tests/smoke.rs](tests/smoke.rs) runs a client and a drone through every layer: messages, packets, COBS frames and
these pieces, over a simulated radio link that loses, damages and splits frames (up to 30% loss). Every request must
be executed and answered exactly once, every event shown once, and nothing may be left to resend. With a tiny
drone outbox (2) it checks the drop rules: only dropped messages may go missing, and nothing is ever shown twice. A
3-second outage checks that IDs still waiting stay within what the other side remembers. What it doesn't simulate yet
is listed at the top of the file.
