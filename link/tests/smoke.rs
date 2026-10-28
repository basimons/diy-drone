//! Smoke test: a client and a drone talk through every layer (Message → Packet → COBS frame → bytes and back, plus
//! the delivery pieces of this crate) over a simulated radio link that loses, damages and splits frames. Runs ~15 s
//! of simulated time per seed, in 1 ms steps, then on until nothing is left to resend.
//!
//! Not simulated (yet): reordering and jitter (a UDP/WiFi link; the radio keeps order), limited bandwidth and the two
//! directions getting different radio time, the drone's Status stream, link-loss timeouts and reconnecting with a new
//! Hello during a session, Hellos crossing, and TakeOff/Flip expiring.

use core::time::Duration;
use drone_common::wire::event::{Armed, BatteryLow, Event, EventId, EventKind};
use drone_common::wire::message::sequences_before;
use drone_common::wire::message::{CLIENT_ID_WINDOW, DRONE_ID_WINDOW, Message, Options, RequestId, Sticks};
use drone_common::wire::message::{Command, CommandMessage, FlightMode, Hello, HelloOptions, HelloReply, HelloResult};
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
/// How bad the radio link is.
#[derive(Clone)]
struct Conditions {
    drop_percent: u64,
    damage_percent: u64,
    outage: core::ops::Range<u64>, // µs: nothing gets through, either way
}

const PERFECT: Conditions = Conditions {
    drop_percent: 0,
    damage_percent: 0,
    outage: 0..0,
};

const LOSSY: Conditions = Conditions {
    drop_percent: 10,
    damage_percent: 3,
    outage: 0..0,
};

const VERY_BAD: Conditions = Conditions {
    drop_percent: 30,
    damage_percent: 5,
    outage: 0..0,
};

/// Lossy, and nothing at all for 3 s in the middle: requests and their answers pile up and wait a long time.
const OUTAGE: Conditions = Conditions {
    outage: 4_000_000..7_000_000,
    ..LOSSY
};

struct Air {
    rng: Rng,
    conditions: Conditions,
    in_flight: VecDeque<(u64, Vec<u8>)>,
    dropped: u32,
    damaged: u32,
}

impl Air {
    fn new(seed: u64, conditions: Conditions) -> Self {
        return Self {
            rng: Rng(seed),
            conditions,
            in_flight: VecDeque::new(),
            dropped: 0,
            damaged: 0,
        };
    }

    fn transmit(&mut self, now: u64, frame: &[u8]) {
        if self.rng.percent(self.conditions.drop_percent) || self.conditions.outage.contains(&now) {
            self.dropped += 1;
            return;
        }
        let mut bytes = frame.to_vec();
        if self.rng.percent(self.conditions.damage_percent) {
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
    damaged_frames: u32,   // frames that arrived but failed COBS or the CRC
    invalid_messages: u32, // intact packets that didn't decode: never, from our own side (a CRC miss would be one)
    skipped: u32,          // gaps the AckTracker counted
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
            invalid_messages: 0,
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
                    Ok(packet) => match Message::decode(&packet) {
                        Ok(message) => messages.push((packet.sequence(), message)),
                        Err(_) => self.invalid_messages += 1,
                    },
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
            self.outbox.on_ack(ack, clock, collect_lost(&mut lost));
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
        self.outbox.check_timeouts(clock, collect_lost(&mut lost));
        self.resend(air, now, lost);
    }

    fn resend(&mut self, air: &mut Air, now: u64, lost: Vec<Message>) {
        for message in lost {
            self.send(air, now, message); // new Sequence, same Request ID or Event ID
        }
    }
}

/// Gathers what the outbox reports lost, to resend once it has returned.
fn collect_lost(lost: &mut Vec<Message>) -> impl FnMut(Delivery<Message>) + '_ {
    return |delivery| {
        if let Delivery::Lost(message) = delivery {
            lost.push(message);
        }
    };
}

/// Whether `older` is `window` or more IDs before `newer`. IDs wrap like Sequences; one past `newer` is never behind.
fn behind_by_window(newer: u16, older: u16, window: usize) -> bool {
    return sequences_before(newer, older).is_some_and(|behind| return usize::from(behind) >= window);
}

struct Client {
    link: Link<DRONE_ID_WINDOW>, // requests waiting are less than DRONE_ID_WINDOW apart, so they always fit
    connected: bool,
    last_hello: Option<u64>,
    next_request: u16,
    answered: RecentIds<RequestId, CLIENT_ID_WINDOW>,
    seen_events: RecentIds<EventId, CLIENT_ID_WINDOW>,
    answers: HashMap<RequestId, u32>,
    events_shown: HashMap<EventId, u32>,
    sticks_sent: u32,
    settled_by_answer: u32, // requests whose answer arrived before their Ack
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
            settled_by_answer: 0,
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
                let settled = self.link.outbox.acknowledge(|sent| {
                    return matches!(sent, Message::Command(command) if command.request_id() == request);
                });
                self.settled_by_answer += u32::from(settled.is_some());
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
        // A new request stays less than DRONE_ID_WINDOW past every request still waiting, so the drone still remembers
        // each one it may get again (protocol.md §5.0a).
        let next = self.next_request;
        let within_window = self.link.outbox.items().all(|sent| {
            return match sent {
                Message::Command(command) => !behind_by_window(next, command.request_id().0, DRONE_ID_WINDOW),
                _ => true,
            };
        });
        if now.is_multiple_of(COMMAND_EVERY_US) && now < QUIET_FROM_US && within_window {
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
    handled: RecentIds<RequestId, DRONE_ID_WINDOW>,
    executed: HashMap<RequestId, u32>,
    next_event: u16,
    sticks_received: u32,
    dropped_events: Vec<EventId>,    // never resent: the client may never see them
    dropped_replies: Vec<RequestId>, // never resent: the client may never get the answer
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
        // Only what the client sends after the HelloReply confirms the session; a Ping may come before (protocol.md §5.1).
        if matches!(message, Message::Sticks(_) | Message::Ack(_) | Message::Command(_)) {
            self.confirmed = true;
        }
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

    /// The drone's rules for Events and answers waiting in its outbox (protocol.md §5.0a): drop those that fell
    /// `CLIENT_ID_WINDOW` behind, and when it is full, drop the oldest Event, or the oldest answer when it holds only
    /// answers, unless the new message is itself an Event: then that one goes.
    fn send_reliable(&mut self, air: &mut Air, now: u64, message: Message) {
        while let Some(old) = self.link.outbox.discard(|sent| return too_far_behind(sent, &message)) {
            self.record_dropped(old);
        }
        if self.link.outbox.is_full() {
            let dropped = match self
                .link
                .outbox
                .discard(|sent| return matches!(sent, Message::Event(_)))
            {
                Some(event) => event,
                None if matches!(message, Message::Event(_)) => {
                    self.record_dropped(message);
                    return;
                }
                None => self
                    .link
                    .outbox
                    .discard(|sent| return matches!(sent, Message::Reply(_)))
                    .unwrap(),
            };
            self.record_dropped(dropped);
        }
        self.link.send(air, now, message);
    }

    fn record_dropped(&mut self, message: Message) {
        match message {
            Message::Event(event) => self.dropped_events.push(event.event_id),
            Message::Reply(reply) => self.dropped_replies.push(reply.request_id()),
            other => panic!("the drone only sends Events and Replies as reliable messages, not {other:?}"),
        }
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

/// Whether `sent`, still waiting, is as far behind `new` (the same kind) as the client remembers IDs.
fn too_far_behind(sent: &Message, new: &Message) -> bool {
    return match (sent, new) {
        (Message::Event(sent), Message::Event(new)) => {
            behind_by_window(new.event_id.0, sent.event_id.0, CLIENT_ID_WINDOW)
        }
        (Message::Reply(sent), Message::Reply(new)) => {
            behind_by_window(new.request_id().0, sent.request_id().0, CLIENT_ID_WINDOW)
        }
        _ => false,
    };
}

struct Run<const N: usize> {
    end_us: u64, // when nothing was left to resend
    client: Client,
    drone: Drone<N>,
    up: Air,   // client → drone
    down: Air, // drone → client
}

fn simulate<const N: usize>(seed: u64, conditions: &Conditions) -> Run<N> {
    let mut client = Client::new();
    let mut drone = Drone::new();
    let mut up = Air::new(seed, conditions.clone());
    let mut down = Air::new(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1, conditions.clone());
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
    assert!(
        drone.next_event > 10,
        "seed {seed}: only {} events sent",
        drone.next_event
    );
    // Only the IDs that were sent show up, nothing decoded into something else.
    assert_eq!(drone.executed.len(), usize::from(requests), "seed {seed}");
    assert!(client.answers.keys().all(|id| return id.0 < requests), "seed {seed}");
    assert!(
        client.events_shown.keys().all(|id| return id.0 < drone.next_event),
        "seed {seed}"
    );
    assert_eq!(
        drone.link.invalid_messages + client.link.invalid_messages,
        0,
        "seed {seed}"
    );
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
    // A bit flipped into 0x00 ends the frame early, so one damaged frame can arrive as two broken ones.
    for (damaged_frames, air) in [(drone.link.damaged_frames, up), (client.link.damaged_frames, down)] {
        assert!((air.damaged..=2 * air.damaged).contains(&damaged_frames), "seed {seed}");
    }
}

#[test]
fn a_perfect_link_delivers_everything() {
    let run = simulate::<32>(1, &PERFECT);
    check(1, &run);
    assert_eq!(run.drone.sticks_received, run.client.sticks_sent);
    assert_eq!(run.drone.link.skipped, 0);
    print_stats(1, &run);
}

#[test]
fn a_lossy_link_still_delivers_every_request_and_event_exactly_once() {
    let mut settled_by_answer = 0;
    for seed in 1..=20 {
        let run = simulate::<32>(seed, &LOSSY);
        check(seed, &run);
        assert_eq!(
            run.drone.dropped_events,
            [],
            "seed {seed}: a normal outbox drops nothing"
        );
        assert_eq!(run.drone.dropped_replies, [], "seed {seed}");
        settled_by_answer += run.client.settled_by_answer;
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
    // protocol.md §5.0a: an answer acknowledges its request too, when the Ack for it was lost.
    assert!(settled_by_answer > 0, "no answer ever arrived before its Ack");
}

#[test]
fn a_very_bad_link_still_delivers() {
    for seed in 1..=5 {
        let run = simulate::<32>(seed, &VERY_BAD);
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
fn a_tiny_drone_outbox_loses_only_what_it_dropped() {
    // An outbox of 2 on a bad link fills up all the time, often with Replies only: both cases of the rule happen.
    // The rule here is the test's own drone; `fc` will need its own test for it.
    let (mut requests, mut unanswered, mut dropped_events, mut dropped_replies) = (0, 0, 0, 0);
    for seed in 1..=10 {
        let run = simulate::<2>(seed, &VERY_BAD);
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
fn a_long_outage_never_runs_or_shows_anything_twice() {
    // While one request or event waits, many newer ones get through; the IDs remembered must still cover it.
    for seed in 1..=40 {
        let run = simulate::<32>(seed, &OUTAGE);
        check(seed, &run);
    }
}
