//! Smoke test: a client and a drone talk through every layer (Message → Packet → COBS frame → bytes and back, plus
//! the delivery pieces of this crate) over a simulated radio link that loses, damages and splits frames. Runs ~15 s
//! of simulated time per seed, in 1 ms steps, then on until nothing is left to resend.

use core::time::Duration;
use drone_common::wire::event::{Armed, BatteryLow, Event, EventId, EventKind};
use drone_common::wire::message::{Command, CommandMessage, FlightMode, Hello, HelloOptions, HelloReply, HelloResult};
use drone_common::wire::message::{Message, Options, RequestId, Sticks};
use drone_common::wire::packet::MAX_PACKET_LEN;
use drone_common::wire::reply::Outcome;
use drone_common::wire::stream::{self, MAX_FRAME_LEN, StreamDecoder};
use drone_link::ack::AckTracker;
use drone_link::outbox::{Delivery, Outbox};
use drone_link::recent_ids::{RecentIds, Seen};
use drone_link::rtt::RttEstimator;
use drone_link::time::SessionTime;
use std::collections::{HashMap, VecDeque};

const PROTOCOL_VERSION: u8 = 1;
const TICK_US: u64 = 1_000;
const LATENCY_US: u64 = 15_000; // one way
const RUN_US: u64 = 15_000_000;
const GIVE_UP_US: u64 = 120_000_000; // still waiting by then: something is stuck
const QUIET_FROM_US: u64 = 10_000_000; // no new commands or events after this, so everything can settle
const HELLO_RESEND_US: u64 = 100_000;
const STICKS_EVERY_US: u64 = 20_000; // 50 Hz
const COMMAND_EVERY_US: u64 = 150_000;
const EVENT_EVERY_US: u64 = 400_000;

/// xorshift64: deterministic, so a failing seed fails the same way every time.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        return self.0;
    }

    fn below(&mut self, n: u64) -> u64 {
        return self.next() % n;
    }

    fn percent(&mut self, chance: u64) -> bool {
        return self.below(100) < chance;
    }
}

/// One direction of the radio link: frames arrive `LATENCY_US` later, unless dropped; some get a bit flipped; bytes
/// come out in chunks of random size, as a serial port hands them over.
struct Air {
    rng: Rng,
    drop_percent: u64,
    damage_percent: u64,
    in_flight: VecDeque<(u64, Vec<u8>)>,
    dropped: u32,
    damaged: u32,
}

impl Air {
    fn new(seed: u64, drop_percent: u64, damage_percent: u64) -> Self {
        return Self {
            rng: Rng(seed),
            drop_percent,
            damage_percent,
            in_flight: VecDeque::new(),
            dropped: 0,
            damaged: 0,
        };
    }

    fn transmit(&mut self, now: u64, frame: &[u8]) {
        if self.rng.percent(self.drop_percent) {
            self.dropped += 1;
            return;
        }
        let mut bytes = frame.to_vec();
        if self.rng.percent(self.damage_percent) {
            let index = self.rng.below(bytes.len() as u64 - 1) as usize; // not the delimiter
            bytes[index] ^= 1 << self.rng.below(8);
            self.damaged += 1;
        }
        self.in_flight.push_back((now + LATENCY_US, bytes));
    }

    fn receive(&mut self, now: u64) -> Vec<Vec<u8>> {
        let mut bytes = Vec::new();
        while self
            .in_flight
            .front()
            .is_some_and(|(arrives, _)| return *arrives <= now)
        {
            bytes.extend(self.in_flight.pop_front().unwrap().1);
        }
        let mut chunks = Vec::new();
        while !bytes.is_empty() {
            let size = (1 + self.rng.below(64) as usize).min(bytes.len());
            chunks.push(bytes.drain(..size).collect());
        }
        return chunks;
    }
}

/// The protocol layers one side runs, as `fc` or `client` would put them together.
struct Link<const N: usize> {
    session_start: u64,
    next_sequence: u16,
    acks: AckTracker,
    outbox: Outbox<Message, N>,
    decoder: StreamDecoder,
    damaged_frames: u32, // frames that arrived but failed COBS or the CRC
    skipped: u32,        // gaps the AckTracker counted
}

impl<const N: usize> Link<N> {
    fn new() -> Self {
        return Self {
            session_start: 0,
            next_sequence: 0,
            acks: AckTracker::new(),
            outbox: Outbox::new(RttEstimator::default()),
            decoder: StreamDecoder::new(),
            damaged_frames: 0,
            skipped: 0,
        };
    }

    /// A new session: sequence counters, acknowledging and resending start over (protocol.md §5.1).
    fn restart(&mut self, now: u64) {
        self.session_start = now;
        self.next_sequence = 0;
        self.acks = AckTracker::new();
        self.outbox = Outbox::new(RttEstimator::default());
    }

    fn clock(&self, now: u64) -> SessionTime {
        return SessionTime(u32::try_from(now - self.session_start).unwrap());
    }

    fn send(&mut self, air: &mut Air, now: u64, message: Message) {
        assert!(
            !(message.is_reliable() && self.outbox.is_full()),
            "outbox full: {message:?}"
        );
        let sequence = self.next_sequence;
        self.next_sequence = sequence.wrapping_add(1);
        if message.is_reliable() {
            self.outbox.send(sequence, message, self.clock(now)).unwrap();
        }
        let mut packet = [0; MAX_PACKET_LEN];
        let mut frame = [0; MAX_FRAME_LEN];
        air.transmit(now, stream::encode(&message.encode(sequence, &mut packet), &mut frame));
    }

    /// Every intact message that arrived by `now`, with its Sequence.
    fn receive(&mut self, air: &mut Air, now: u64) -> Vec<(u16, Message)> {
        let mut messages = Vec::new();
        for chunk in air.receive(now) {
            let mut chunk = &chunk[..];
            while let Some(packet) = self.decoder.next_packet(&mut chunk) {
                match packet {
                    Ok(packet) => {
                        let message = Message::decode(&packet).expect("an intact packet from our own side decodes");
                        messages.push((packet.sequence(), message));
                    }
                    Err(_) => self.damaged_frames += 1,
                }
            }
        }
        return messages;
    }

    /// Records an arrival in the session; an Ack settles what it covers, and what it shows lost is resent.
    fn arrived(&mut self, air: &mut Air, now: u64, sequence: u16, message: &Message) {
        let clock = self.clock(now);
        if let drone_link::ack::Arrival::Newest { skipped } =
            self.acks.on_receive(sequence, message.is_reliable(), clock)
        {
            self.skipped += u32::from(skipped);
        }
        if let Message::Ack(ack) = message {
            let mut lost = Vec::new();
            self.outbox.on_ack(ack, clock, |delivery| {
                if let Delivery::Lost(message) = delivery {
                    lost.push(message);
                }
            });
            self.resend(air, now, lost);
        }
    }

    /// The timers: the Ack when it is due, resending what timed out.
    fn tick(&mut self, air: &mut Air, now: u64) {
        let clock = self.clock(now);
        if self.acks.ack_due(clock) == Some(Duration::ZERO) {
            let ack = self.acks.ack(clock).unwrap();
            self.send(air, now, Message::Ack(ack));
        }
        let mut lost = Vec::new();
        self.outbox.check_timeouts(clock, |delivery| {
            if let Delivery::Lost(message) = delivery {
                lost.push(message);
            }
        });
        self.resend(air, now, lost);
    }

    fn resend(&mut self, air: &mut Air, now: u64, lost: Vec<Message>) {
        for message in lost {
            self.send(air, now, message); // new Sequence, same Request ID or Event ID
        }
    }
}

struct Client {
    link: Link<32>, // the client never has more than 16 requests waiting
    connected: bool,
    last_hello: Option<u64>,
    next_request: u16,
    answered: RecentIds<RequestId, 16>,
    seen_events: RecentIds<EventId, 16>,
    answers: HashMap<RequestId, u32>,
    events_shown: HashMap<EventId, u32>,
    sticks_sent: u32,
}

impl Client {
    fn new() -> Self {
        return Self {
            link: Link::new(),
            connected: false,
            last_hello: None,
            next_request: 0,
            answered: RecentIds::new(),
            seen_events: RecentIds::new(),
            answers: HashMap::new(),
            events_shown: HashMap::new(),
            sticks_sent: 0,
        };
    }

    fn receive(&mut self, air: &mut Air, now: u64, (sequence, message): (u16, Message)) {
        if !self.connected {
            // Until the handshake succeeds, only the HelloReply counts (protocol.md §5.1).
            if let Message::HelloReply(reply) = message {
                assert_eq!(reply.result, HelloResult::Ok);
                self.connected = true;
                self.link.arrived(air, now, sequence, &message);
            }
            return;
        }
        self.link.arrived(air, now, sequence, &message);
        match message {
            Message::Reply(reply) => {
                let request = reply.request_id();
                self.link.outbox.acknowledge(|sent| {
                    return matches!(sent, Message::Command(command) if command.request_id() == request);
                });
                match self.answered.remember(request) {
                    Seen::New => *self.answers.entry(request).or_default() += 1,
                    Seen::Repeat => {} // the Reply was resent: its Ack got lost
                }
            }
            Message::Event(event) => {
                match self.seen_events.remember(event.event_id) {
                    Seen::New => *self.events_shown.entry(event.event_id).or_default() += 1,
                    Seen::Repeat => {} // a resend: shown already
                }
            }
            _ => {}
        }
    }

    fn act(&mut self, air: &mut Air, now: u64) {
        if !self.connected {
            if self.last_hello.is_none_or(|last| return now - last >= HELLO_RESEND_US) {
                self.link.restart(now); // the client's session clock starts at its Hello
                let hello = Hello {
                    protocol_version: PROTOCOL_VERSION,
                    options: HelloOptions::default(),
                };
                self.link.send(air, now, Message::Hello(hello));
                self.last_hello = Some(now);
            }
            return;
        }
        if now.is_multiple_of(STICKS_EVERY_US) {
            let sticks = Sticks {
                throttle: (now / 1_000) as u16,
                roll: 0,
                pitch: 0,
                yaw: 0,
            };
            self.link.send(air, now, Message::Sticks(sticks));
            self.sticks_sent += 1;
        }
        // At most 16 requests waiting at once: the drone remembers 16 Request IDs (protocol.md §5.0a).
        if now.is_multiple_of(COMMAND_EVERY_US) && now < QUIET_FROM_US && self.link.outbox.len() < 16 {
            let command = match self.next_request % 3 {
                0 => Command::Arm,
                1 => Command::SetFlightMode {
                    mode: FlightMode::Angle,
                },
                _ => Command::Disarm,
            };
            let sent_at = self.link.clock(now).0;
            let message = CommandMessage::new(RequestId(self.next_request), sent_at, command);
            self.link.send(air, now, Message::Command(message));
            self.next_request = self.next_request.wrapping_add(1);
        }
    }
}

struct Drone<const N: usize> {
    link: Link<N>,
    connected: bool,
    confirmed: bool, // the client's first message after its Hello arrived: it has the HelloReply
    handled: RecentIds<RequestId, 16>,
    executed: HashMap<RequestId, u32>,
    next_event: u16,
    sticks_received: u32,
    dropped_events: Vec<EventId>, // discarded from a full outbox: the client may never see them
    dropped_replies: Vec<RequestId>, // the same, for Replies: the client may never get the answer
}

impl<const N: usize> Drone<N> {
    fn new() -> Self {
        return Self {
            link: Link::new(),
            connected: false,
            confirmed: false,
            handled: RecentIds::new(),
            executed: HashMap::new(),
            next_event: 0,
            sticks_received: 0,
            dropped_events: Vec::new(),
            dropped_replies: Vec::new(),
        };
    }

    fn receive(&mut self, air: &mut Air, now: u64, (sequence, message): (u16, Message)) {
        if let Message::Hello(hello) = message {
            assert_eq!(hello.protocol_version, PROTOCOL_VERSION);
            self.link.restart(now); // the drone's session clock starts when the Hello arrives
            self.handled = RecentIds::new();
            self.connected = true;
            self.confirmed = false;
            self.link.arrived(air, now, sequence, &message);
            let reply = HelloReply {
                protocol_version: PROTOCOL_VERSION,
                result: HelloResult::Ok,
                options: Options::default(),
            };
            self.link.send(air, now, Message::HelloReply(reply));
            return;
        }
        if !self.connected {
            return;
        }
        self.confirmed = true;
        self.link.arrived(air, now, sequence, &message);
        match message {
            Message::Sticks(_) => self.sticks_received += 1,
            Message::Command(command) => {
                match self.handled.remember(command.request_id()) {
                    Seen::New => {
                        *self.executed.entry(command.request_id()).or_default() += 1;
                        self.send_reliable(air, now, Message::Reply(command.reply(Outcome::Ok)));
                    }
                    Seen::Repeat => {} // acknowledged above; its Reply is resent by the outbox until acknowledged
                }
            }
            _ => {}
        }
    }

    /// A full outbox makes room: the oldest Event goes first, the oldest Reply only when it holds nothing else
    /// (protocol.md §5.0a).
    fn send_reliable(&mut self, air: &mut Air, now: u64, message: Message) {
        if self.link.outbox.is_full() {
            let dropped = self
                .link
                .outbox
                .discard(|sent| return matches!(sent, Message::Event(_)))
                .or_else(|| {
                    return self
                        .link
                        .outbox
                        .discard(|sent| return matches!(sent, Message::Reply(_)));
                });
            match dropped {
                Some(Message::Event(event)) => self.dropped_events.push(event.event_id),
                Some(Message::Reply(reply)) => self.dropped_replies.push(reply.request_id()),
                other => panic!("the drone's outbox only holds Events and Replies, not {other:?}"),
            }
        }
        self.link.send(air, now, message);
    }

    fn act(&mut self, air: &mut Air, now: u64) {
        // Only once the session is confirmed: if the HelloReply was lost, the client's next Hello starts a new session
        // and drops everything unacknowledged from this one (protocol.md §5.1).
        if self.confirmed && now.is_multiple_of(EVENT_EVERY_US) && now < QUIET_FROM_US {
            let kind = if self.next_event.is_multiple_of(2) {
                EventKind::Armed(Armed)
            } else {
                EventKind::BatteryLow(BatteryLow { millivolts: 14_200 })
            };
            let event = Event {
                event_id: EventId(self.next_event),
                kind,
            };
            self.send_reliable(air, now, Message::Event(event));
            self.next_event = self.next_event.wrapping_add(1);
        }
    }
}

struct Run<const N: usize> {
    end_us: u64, // when nothing was left to resend
    client: Client,
    drone: Drone<N>,
    up: Air,   // client → drone
    down: Air, // drone → client
}

fn simulate<const N: usize>(seed: u64, drop_percent: u64, damage_percent: u64) -> Run<N> {
    let mut client = Client::new();
    let mut drone = Drone::new();
    let mut up = Air::new(seed, drop_percent, damage_percent);
    let mut down = Air::new(
        seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1,
        drop_percent,
        damage_percent,
    );
    let mut now = 0;
    while now < RUN_US || !(client.link.outbox.is_empty() && drone.link.outbox.is_empty()) {
        assert!(
            now < GIVE_UP_US,
            "seed {seed}: still resending after {} s",
            GIVE_UP_US / 1_000_000
        );
        for arrived in client.link.receive(&mut down, now) {
            client.receive(&mut up, now, arrived);
        }
        for arrived in drone.link.receive(&mut up, now) {
            drone.receive(&mut down, now, arrived);
        }
        client.act(&mut up, now);
        drone.act(&mut down, now);
        client.link.tick(&mut up, now);
        drone.link.tick(&mut down, now);
        now += TICK_US;
    }
    return Run {
        end_us: now,
        client,
        drone,
        up,
        down,
    };
}

/// Everything protocol.md §5.0a promises: each request executed once and answered once, each event shown once,
/// nothing left waiting at the end.
fn check<const N: usize>(seed: u64, run: &Run<N>) {
    let Run {
        client,
        drone,
        up,
        down,
        ..
    } = run;
    let requests = client.next_request;
    assert!(requests > 50, "seed {seed}: only {requests} requests sent");
    for id in (0..requests).map(RequestId) {
        assert_eq!(drone.executed.get(&id), Some(&1), "seed {seed}: {id:?} executed");
        // A dropped Reply may still have arrived before it was dropped; then it is shown once too.
        let answers = client.answers.get(&id).copied().unwrap_or(0);
        let expected = if drone.dropped_replies.contains(&id) {
            0..=1
        } else {
            1..=1
        };
        assert!(
            expected.contains(&answers),
            "seed {seed}: {id:?} answered {answers} times"
        );
    }
    for id in (0..drone.next_event).map(EventId) {
        let shown = client.events_shown.get(&id).copied().unwrap_or(0);
        let expected = if drone.dropped_events.contains(&id) {
            0..=1
        } else {
            1..=1
        };
        assert!(expected.contains(&shown), "seed {seed}: {id:?} shown {shown} times");
    }
    assert!(
        client.link.outbox.is_empty(),
        "seed {seed}: the client still waits for Acks"
    );
    assert!(
        drone.link.outbox.is_empty(),
        "seed {seed}: the drone still waits for Acks"
    );
    // Damaged frames are dropped, never decoded into something else (`receive` would panic). A bit flipped into 0x00
    // ends the frame early, so one damaged frame can arrive as two broken ones.
    for (damaged_frames, air) in [(drone.link.damaged_frames, up), (client.link.damaged_frames, down)] {
        assert!((air.damaged..=2 * air.damaged).contains(&damaged_frames), "seed {seed}");
    }
}

#[test]
fn a_perfect_link_delivers_everything() {
    let run = simulate::<32>(1, 0, 0);
    check(1, &run);
    assert_eq!(run.drone.sticks_received, run.client.sticks_sent);
    assert_eq!(run.drone.link.skipped, 0);
    print_stats(1, &run);
}

#[test]
fn a_lossy_link_still_delivers_every_request_and_event_exactly_once() {
    for seed in 1..=20 {
        let run = simulate::<32>(seed, 10, 3);
        check(seed, &run);
        assert!(
            run.up.dropped > 0 && run.down.dropped > 0 && run.up.damaged > 0,
            "seed {seed}: no loss happened"
        );
        // Every lost or damaged frame shows up as a gap; Sticks are never resent.
        let sticks_lost = run.client.sticks_sent - run.drone.sticks_received;
        assert!(
            sticks_lost > 0 && sticks_lost <= run.up.dropped + run.up.damaged,
            "seed {seed}"
        );
        if seed == 1 {
            print_stats(seed, &run);
        }
    }
}

#[test]
fn a_very_bad_link_still_delivers() {
    for seed in 1..=5 {
        let run = simulate::<32>(seed, 30, 5);
        check(seed, &run);
        print_stats(seed, &run);
    }
}

fn print_stats<const N: usize>(seed: u64, run: &Run<N>) {
    let Run {
        client,
        drone,
        up,
        down,
        ..
    } = run;
    println!(
        "seed {seed}: {} requests, {} events, sticks {}/{} arrived; up: {} dropped, {} damaged; down: {} dropped, {} \
         damaged; drone counted {} gaps; RTT client {:?}, drone {:?}; settled at {} ms",
        client.next_request,
        drone.next_event,
        drone.sticks_received,
        client.sticks_sent,
        up.dropped,
        up.damaged,
        down.dropped,
        down.damaged,
        drone.link.skipped,
        client.link.outbox.rtt().smoothed(),
        drone.link.outbox.rtt().smoothed(),
        run.end_us / 1_000,
    );
}

#[test]
fn a_full_drone_outbox_drops_events_before_replies() {
    // An outbox of 2 on a bad link fills up all the time, often with Replies only: both cases of the rule happen.
    let (mut requests, mut unanswered, mut dropped_events, mut dropped_replies) = (0, 0, 0, 0);
    for seed in 1..=10 {
        let run = simulate::<2>(seed, 30, 5);
        check(seed, &run);
        requests += usize::from(run.client.next_request);
        unanswered += (0..run.client.next_request)
            .filter(|&id| return !run.client.answers.contains_key(&RequestId(id)))
            .count();
        dropped_events += run.drone.dropped_events.len();
        dropped_replies += run.drone.dropped_replies.len();
    }
    println!(
        "outbox of 2, 10 seeds: {requests} requests, {unanswered} never answered; {dropped_events} Events and \
         {dropped_replies} Replies dropped"
    );
    assert!(
        dropped_events > 0 && dropped_replies > 0,
        "both cases of the rule should happen"
    );
}

#[test]
fn a_normal_drone_outbox_drops_nothing() {
    for seed in 1..=20 {
        let run = simulate::<32>(seed, 10, 3);
        assert_eq!(run.drone.dropped_events, [], "seed {seed}");
        assert_eq!(run.drone.dropped_replies, [], "seed {seed}");
    }
}
