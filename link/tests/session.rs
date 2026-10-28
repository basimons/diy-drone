//! The pieces together: the example of protocol.md §5.0, where the answer to a request and the Ack for it are lost.

use drone_common::wire::message::{Command, CommandMessage, Message, RequestId};
use drone_common::wire::reply::Outcome;
use drone_link::ack::AckTracker;
use drone_link::outbox::{Delivery, Outbox};
use drone_link::recent_ids::{RecentIds, Seen};
use drone_link::rtt::RttEstimator;
use drone_link::time::SessionTime;

/// One side of the link, as `fc` or `client` would combine the pieces. Sent messages pile up in `sent` until the
/// test delivers or drops them.
struct Side {
    next_sequence: u16,
    acks: AckTracker,
    outbox: Outbox<Message, 16>,
    handled: RecentIds<RequestId, 16>, // the drone: requests it executed; the client: requests it got answers for
    sent: Vec<(u16, Message)>,
}

impl Side {
    fn new() -> Self {
        return Self {
            next_sequence: 0,
            acks: AckTracker::new(),
            outbox: Outbox::new(RttEstimator::default()),
            handled: RecentIds::new(),
            sent: Vec::new(),
        };
    }

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
        self.sent.push((sequence, message));
        return Ok(());
    }

    /// What arrived, as the test hands it over; returns the messages that are new to act on.
    fn receive(&mut self, (sequence, message): (u16, Message), now: SessionTime) -> Option<Message> {
        self.acks.on_receive(sequence, message.is_reliable(), now);
        return match message {
            Message::Ack(ack) => {
                let mut lost = Vec::new();
                self.outbox.on_ack(&ack, now, |delivery| {
                    if let Delivery::Lost(message) = delivery {
                        lost.push(message);
                    }
                });
                self.resend(lost, now);
                None
            }
            Message::Command(command) => match self.handled.remember(command.request_id()) {
                Seen::New => Some(message),
                Seen::Repeat => None, // acknowledged above, not executed twice
            },
            Message::Reply(reply) => {
                // An answer acknowledges its request too (protocol.md §5.0a).
                let request = reply.request_id();
                self.outbox.acknowledge(|sent| {
                    return matches!(sent, Message::Command(command) if command.request_id() == request);
                });
                match self.handled.remember(request) {
                    Seen::New => Some(message),
                    Seen::Repeat => None,
                }
            }
            _ => Some(message),
        };
    }

    /// What a timer in `fc` / `client` would do: send the Ack when due, resend what timed out.
    fn tick(&mut self, now: SessionTime) {
        if self.acks.ack_due(now) == Some(core::time::Duration::ZERO) {
            let ack = self.acks.ack(now).unwrap();
            self.send(Message::Ack(ack), now).unwrap();
        }
        let mut lost = Vec::new();
        self.outbox.check_timeouts(now, |delivery| {
            if let Delivery::Lost(message) = delivery {
                lost.push(message);
            }
        });
        self.resend(lost, now);
    }

    fn resend(&mut self, lost: Vec<Message>, now: SessionTime) {
        for message in lost {
            self.send(message, now).unwrap(); // new Sequence, same Request ID
        }
    }

    fn take_sent(&mut self) -> Vec<(u16, Message)> {
        return std::mem::take(&mut self.sent);
    }
}

fn at(ms: u32) -> SessionTime {
    return SessionTime(ms * 1_000);
}

/// Hands everything `from` sent to `to`; returns what `to` acts on.
fn deliver(from: &mut Side, to: &mut Side, now: SessionTime) -> Vec<Message> {
    return from
        .take_sent()
        .into_iter()
        .filter_map(|sent| return to.receive(sent, now))
        .collect();
}

#[test]
fn a_request_whose_answer_and_ack_are_lost_is_executed_once_and_answered_once() {
    let mut client = Side::new();
    let mut drone = Side::new();
    let arm = Message::Command(CommandMessage::new(RequestId(7), 0, Command::Arm));

    client.send(arm, at(0)).unwrap();
    assert_eq!(deliver(&mut client, &mut drone, at(5)), [arm]);
    let Message::Command(request) = arm else { unreachable!() };
    drone.send(Message::Reply(request.reply(Outcome::Ok)), at(6)).unwrap();
    drone.tick(at(25)); // the Ack is due 20 ms after the request arrived
    assert_eq!(drone.take_sent().len(), 2); // the Reply and the Ack: both lost

    client.tick(at(320)); // no Ack within the probe timeout
    let resent = client.take_sent();
    assert_eq!(resent.len(), 1);
    assert_ne!(resent[0].0, 0, "a resend gets a new Sequence");
    assert_eq!(resent[0].1, arm, "and keeps its Request ID");
    let acted_on: Vec<_> = resent
        .into_iter()
        .filter_map(|sent| return drone.receive(sent, at(330)))
        .collect();
    assert_eq!(acted_on, [], "the drone doesn't arm twice");

    drone.tick(at(350)); // Ack for the resend; the Reply, unacknowledged since 6 ms, times out too
    let answers = deliver(&mut drone, &mut client, at(355));
    assert!(matches!(answers[..], [Message::Reply(reply)] if reply.request_id() == RequestId(7)));
    assert!(client.outbox.is_empty(), "the Reply settled the request");

    client.tick(at(380)); // Ack for the Reply
    deliver(&mut client, &mut drone, at(385));
    assert!(drone.outbox.is_empty(), "the client acknowledged the Reply");
}
