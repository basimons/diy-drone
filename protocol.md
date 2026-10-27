# Link Protocol

*2026-10-22. Status: being implemented; §7 describes the current code.*

## 1. Intent

The drone and the client talk over a byte link. We design **our own protocol** for that link, for fun and for learning, instead of being limited to what ELRS's CRSF format can carry.

The protocol must run over any plain byte pipe:
- **ELRS in Airport mode** (the radios become a transparent serial link) — the main use.
- A **USB cable** between laptop and Pi — for bench testing without radios.
- **WiFi** (later) — e.g. fast log download on the ground.

At the same time the drone must still be flyable with **standard ELRS/CRSF** (normal, non-Airport mode). Both protocols are translated into one **internal state**; the flight code only ever sees that state.

## 2. Goals, non-goals, anti-goals

**Goals**
- One protocol covering control, commands, settings, telemetry, events and log download.
- **Never run out of room**: unused type numbers, reserved flag bits and option bits so new things can be added.
- Byte layout fully owned and hand-written by us (no serialization library deciding it).
- Usable with no dynamic memory (fixed buffers), so the flight side can use it safely.
- Room to add **authentication** later without redesigning the format.

**Non-goals (for now)**
- **Backwards compatibility.** Breaking older code on both sides is fine; that is what the protocol version is for. Adding a message type, code, event or reason is a new protocol version too, so after a successful Hello both sides know exactly the same codes. Only Hello and HelloReply are read across versions (§5.1).
- Authentication/signing itself (only the room for it).
- Deciding *when the drone accepts or refuses* something (e.g. "settings only while disarmed", "no flip below X m"). That is drone-side logic, built later. The protocol only provides the way to say "refused, because…".
- Encryption (hiding content). Nothing sent is secret.

**Anti-goals**
- A format where adding a message or setting forces a redesign.
- A stale or frozen client that the drone cannot detect (see §6 rule 4).

## 3. Architecture: adapters and the internal state

```
 ELRS receiver ──CRSF bytes──► CRSF adapter ──┐
                                              ├──► internal state ──► flight code
 Airport/USB/WiFi ──our bytes──► our adapter ─┘         │
                                                        ▼
                             status ──► each adapter encodes what its protocol can carry
```

- Each protocol has an **adapter** that decodes its bytes into the internal state and encodes drone status back out.
- The internal state types live in `drone-common`, so the client uses the same types.
- The internal state can hold more than CRSF can express (settings, flips with parameters, log requests). Over CRSF those simply never arrive; that is fine.
- **Link health is part of the state**: each adapter records when the last valid message arrived, so link-loss handling is identical whatever protocol is in use.
- **One active source at a time**, chosen at startup.
- Over CRSF, simple commands can be mapped to switch channels (e.g. "channel 8 high" → flip). The exact mapping is still to be decided (§9).

## 4. Framing

### 4.0 Notation

Message layouts use the packet diagram style of RFCs, following the [Augmented Packet Header Diagrams](https://www.ietf.org/archive/id/draft-mcquistin-augmented-ascii-diagrams-13.html) draft: a diagram, then a "where:" list giving each field's name, short name, size and constraints.

- Each row of a diagram is 32 bits (4 bytes). The top line of the ruler numbers the bytes in a row (0–3), the line below the bits within each byte (0–7).
- A field drawn with `:` edges is **variable length**: it takes the remaining bytes (or the length given by an earlier field).
- `(cont.)` means the field above continues on the next row.
- **Byte order: all multi-byte numbers are little-endian** (low byte first). This deliberately differs from the RFC convention of big-endian ("network byte order").
- `u8/u16/u32` = unsigned integers of 1/2/4 bytes, `i16` = signed 2-byte integer, `f32` = 4-byte IEEE 754 floating-point number.

### 4.1 Packet

Every message travels as one packet (on byte streams it is then COBS-framed, §4.2):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|     Flags     |      Type     |            Sequence           |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                       Payload (variable)                      :
:                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                 Auth Tag (only if Flags bit 0)                :
:                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|             CRC-16            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Flags (F):** 1 byte. Bit 0 = "auth tag present". Bits 1–7 reserved. For now F == 0; until authentication exists, a receiver drops any packet with F != 0.
- **Type (T):** 1 byte. Message type, see §5. 256 values; if more are ever needed, that is a new protocol version.
- **Sequence (Seq):** 2 bytes, u16. Counter per direction, +1 for every message sent (resends too), wraps 65535 → 0. Detects lost messages (gaps), a frozen sender (counter stops), stale messages (over WiFi: a message that may be lost, §5.0a, older than the newest seen → dropped), and later replay. The receiver reports which Sequences arrived with Ack (§5.0a). It is **not** used to re-sort messages and **not** used to match answers to requests (that is the Request ID, §5.0).
- **Payload:** variable length, layout depends on T (§5). Requests and their answers start their payload with a Request ID (§5.0); other messages (Sticks, Hello, Ping, Event, …) don't have one.
- **Auth Tag:** present only when F bit 0 is set. Length and algorithm defined by a future protocol version. Absent for now.
- **CRC-16 (CRC):** 2 bytes. Over every byte before it (Flags through Auth Tag). Variant CRC-16/CCITT-FALSE: polynomial 0x1021, init 0xFFFF, no reflection, no final XOR.

Constraint: the whole packet is at most **255 bytes** (deliberate: fixed buffers, small radio packets; raise in a later version if needed). With the 6 bytes of Flags, Type, Sequence and CRC, a payload is at most 249 bytes.

### 4.2 Byte streams: COBS

How packets travel depends on the link:

- **Packet links (WiFi/UDP):** one datagram carries exactly one packet, as is. The datagram boundary marks where it ends.
- **Byte streams (serial, radio):** the packet is COBS-encoded (Consistent Overhead Byte Stuffing) so it contains no `0x00` bytes, then followed by a single `0x00` delimiter. That is a **frame**.
  - A receiver that loses sync just waits for the next `0x00`.
  - Packet length is known from the delimiter position, so there is no length field.
  - Packets ≤ 254 bytes cost exactly 1 extra byte, plus the delimiter.

**Receive rule:** on a stream, cut at `0x00` and undo COBS. Then, on every link: check 6 ≤ length ≤ 255, check CRC, check Flags. Anything that fails is dropped silently (counted for link-quality stats).

**Example size:** a Sticks message is 6 (packet header and CRC) + 8 (payload) = 14 bytes as a packet, + 2 (COBS + delimiter) = **16 bytes** as a frame on a byte stream.

## 5. Message types

`0x00` is never used. Ranges group topics; `0x80–0xFF` are free.

| Range | Topic | Types |
|-------|-------|-------|
| `0x01–0x0F` | Connection | `0x01` Hello, `0x02` HelloReply, `0x03` Ping, `0x04` Pong, `0x05` LinkTest, `0x06` LinkTestData, `0x07` LinkTestResult, `0x08` Ack |
| `0x10–0x1F` | Control | `0x10` Sticks |
| `0x20–0x3F` | Commands | `0x20` Arm, `0x21` Disarm, `0x22` TakeOff, `0x23` Land, `0x24` GoHome, `0x25` Flip, `0x26` SteadyState, `0x27` ExitSteadyState |
| `0x40–0x4F` | Replies & events | `0x40` Reply, `0x41` Event |
| `0x50–0x5F` | Settings | `0x50` SetSettings, `0x51` GetSettings, `0x52` Settings |
| `0x60–0x6F` | Data streams | `0x60` RequestOnce, `0x61` Subscribe, `0x62` StreamData, `0x63` StreamUpdate |
| `0x70–0x7F` | Logs | `0x70` ListLogs, `0x71` LogList, `0x72` ReadLog, `0x73` LogChunk |

Direction: C→D = client (laptop) to drone, D→C = drone to client. The diagrams below show the **payload** only; every message is wrapped in the packet of §4.1.

### 5.0 Requests and answers

- A **request** is any C→D message except Sticks, Hello, Ping and LinkTestData: commands, LinkTest, SetSettings, GetSettings, Subscribe, RequestOnce, ListLogs, ReadLog.
- Every request payload **starts with a Request ID (u16)**, the client picks it by counting up its requests (wrapping 65535 → 0; every value is valid). It tells the drone "this is the same request as before" and lets the client match answers to requests. It is separate from the packet Sequence.
- Every request gets **exactly one answer**: its data message (LinkTestResult, Settings, StreamData, LogList, LogChunk, which echo the Request ID), or a Reply. Errors (unknown stream/group/log ID, bad rate, refused) are always a Reply.
- **Resending** works the same way in both directions (§5.0a). A resent request keeps its **Request ID** and gets a new Sequence. The drone remembers the last 16 Request IDs it handled; a repeat is acknowledged but not executed twice.
- Data the drone sends without being asked (default and subscribed streams) is a **StreamUpdate**, which has no Request ID. Whether data answers a request is told by its message type, not by a special Request ID value.
- The Request ID is 2 bytes (not 1) so that many requests in flight (e.g. pipelined ReadLog over WiFi) never wrap into IDs the drone still remembers.

Example:

```
C→D seq 101  Flip, Request ID 7
D→C seq 40   Reply to Request ID 7: OK   ← lost
D→C seq 41   Ack, Largest Received 101   ← lost
C→D seq 106  Flip, Request ID 7          ← resend: new Sequence, same Request ID
             drone: already did 7 → acknowledges 106, does not flip twice
D→C seq 43   Reply to Request ID 7: OK   ← the drone resends its Reply too: seq 40 was never acknowledged
```

### 5.0a Delivery: acknowledgements and resending

This works like QUIC (the protocol under HTTP/3), not like TCP: nothing waits for a lost message, and a resend is a new packet with a new Sequence, so every Sequence is sent only once. **Both sides follow the same rules.**

- **Reliable messages** must arrive: requests, their answers (Reply and the data messages that answer a request) and Events. The sender keeps each one until it is acknowledged, and resends it when it is lost.
- **Everything else may be lost** and is never resent: Sticks, StreamUpdate, Ping, Pong, LinkTestData and Ack. The next one replaces it; for LinkTestData the losses are what is being measured. Hello and HelloReply are the exception: the client resends Hello until a HelloReply arrives.
- **Acknowledging:** a side that receives a reliable message sends an **Ack** (below) within 20 ms. One Ack covers every Sequence received so far, reliable or not, so one Ack can acknowledge several messages. An Ack is only sent when a reliable message is waiting to be acknowledged. Acks are never acknowledged or resent: the next Ack repeats what a lost one said.
- **An answer acknowledges its request too**, so the client stops resending a request once its answer arrives, even if the Ack was lost.
- **Lost:** the sender counts a reliable message as lost when
  - a message sent 3 or more Sequences after it has been acknowledged (on a byte stream messages arrive in order; over UDP they may not, and 3 leaves room for that), or
  - no Ack covers it within the **probe timeout**: smoothed RTT + max(4 × RTT variation, 1 ms) + 20 ms. Each probe timeout in a row doubles it; an Ack resets it.
- **Resending:** a lost message is sent again with a new Sequence and its ID unchanged: the Request ID, or the Event ID of an Event. The receiver recognises the ID and doesn't act twice: the drone remembers the last 16 Request IDs, the client the last 16 Event IDs and the Request IDs it got answers for.
- **Round-trip time (RTT):** when an Ack's Largest Received is a reliable message not acknowledged before, RTT sample = now − when that message was sent − Ack Delay. A resend has its own Sequence, so a sample can't be mistaken for an earlier copy (TCP's problem). As in QUIC (RFC 9002): smoothed RTT = ⅞ smoothed RTT + ⅛ sample, RTT variation = ¾ RTT variation + ¼ |smoothed RTT − sample|. Until the first sample: the Ping/Pong round trip if there is one, else 100 ms.
- **At most 16 unacknowledged requests at once.** The drone remembers 16 Request IDs, so a request the client may still resend is always among them and is never executed twice.
- **Giving up:** TakeOff and Flip expire after 300 ms (§5.3). Everything else is resent until it is acknowledged, the link is declared lost (§6 rule 4), or a new Hello starts a new session; nothing from an old session is resent.

**Ack** (`0x08`, either direction):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|        Largest Received       |           Ack Delay           |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                        Received Before                        |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Largest Received (LR):** 2 bytes, u16. The newest Sequence received from the other side (compared with wrapping arithmetic).
- **Ack Delay (AD):** 2 bytes, u16, in µs. Time from receiving Largest Received to sending this Ack. The other side subtracts it from its RTT sample.
- **Received Before (RB):** 4 bytes, u32, bit field. Bit n (bit 0 is the lowest) set means Sequence Largest Received − 1 − n arrived too, so one Ack covers 33 Sequences. A reliable message older than that and not yet acknowledged is lost by the rule above.

### 5.1 Connection

**Hello** (`0x01`, C→D):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|    Version    |                    Options                    |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|    (cont.)    |
+-+-+-+-+-+-+-+-+
```

where:

- **Version (Ver):** 1 byte, u8. Protocol version, starts at 1.
- **Options (Opt):** 4 bytes, u32. Options the client wants. Bit 0 = authentication. Bits 1–31 free. For now Opt == 0.

**HelloReply** (`0x02`, D→C):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|    Version    |     Result    |            Options            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|            (cont.)            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Version (Ver):** 1 byte, u8. The drone's protocol version.
- **Result (R):** 1 byte. One of: 0 = OK, 1 = version mismatch, 2 = option not supported.
- **Options (Opt):** 4 bytes, u32. The options the drone accepted. When R == 2, the bits the client asked for that are missing here are the unsupported ones.

**Ping** (`0x03`, either direction):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                           Timestamp                           |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Timestamp (TS):** 4 bytes, u32. Sender's session clock (see "Session clock" below) in µs when the Ping was sent.

**Pong** (`0x04`, the answer to a Ping):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                           Ping Time                           |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                          Receive Time                         |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                           Send Time                           |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Ping Time (T1):** 4 bytes, u32. The Ping's Timestamp, echoed unchanged.
- **Receive Time (T2):** 4 bytes, u32. Answerer's session clock in µs when the Ping arrived.
- **Send Time (T3):** 4 bytes, u32. Answerer's session clock in µs when the Pong was sent.

With T4 = the Ping sender's session clock when the Pong arrives (NTP-style):

- round-trip delay = (T4 − T1) − (T3 − T2)
- clock offset (answerer − sender) ≈ ((T2 − T1) + (T3 − T4)) / 2, assuming the delay is the same both ways

The offset lets drone logs and client logs be lined up afterwards.

**Session clock:** every timestamp (point in time) in the protocol is a u32 count of **microseconds since the start of the session**, not wall-clock or UTC time. Durations, like the Flip duration, keep their own units.

- the client's session clock starts at 0 when it sends the Hello;
- the drone's session clock starts at 0 when it receives that Hello.

So both clocks start at almost the same moment and the offset between them is roughly the one-way delay. A new Hello restarts both clocks. A u32 of µs wraps after about 71 minutes; all differences are computed with wrapping arithmetic, which stays correct as long as the two times are less than ~35 minutes apart.

Rules:

- **Until a handshake succeeds, each side ignores everything except Hello, HelloReply, Ping and Pong.** A Ping before the handshake only measures the round trip; its times use no session clock yet.
- **Hello and HelloReply carry Sequence 0.** On a valid Hello the drone resets both counters; its HelloReply is its message 0.
- **Hello and HelloReply never change**, not even in a new protocol version: their layout, Result codes and the meaning of each option bit stay as they are, so a client and drone of different versions can always tell each other so. Option bits the receiver doesn't know are kept, so the drone can answer "option not supported".
- **A new Hello restarts the session at the protocol layer only:** sequence counters, subscriptions (back to the default set) and the remembered Request IDs. It does **not** touch the internal state (sticks, armed state, settings). Sticks stop arriving while the client reconnects; that gap is handled by the normal link-loss logic like any other silence.
- When authentication is added, the handshake is also where both sides exchange fresh random numbers for the session (protection against replaying recorded messages). Not now.

### 5.1a Link test

Measures how much data the link can carry, per direction (ELRS splits radio time very unevenly between the two directions, so they can differ a lot). Meant for the ground, or later in steady state (§5.3); when it is allowed is drone-side logic.

**LinkTest** (`0x05`, C→D):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           Request ID          |   Direction   |    Duration   |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|    (cont.)    |  Filler Size  |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Request ID (RID):** 2 bytes, u16.
- **Direction (Dir):** 1 byte. One of: 0 = drone sends, 1 = client sends.
- **Duration (Dur):** 2 bytes, u16, in ms. How long the sender keeps sending (up to ~65 s).
- **Filler Size (FS):** 1 byte, 0 … 247. Bytes of filler per LinkTestData message, to see how message size affects throughput.

**LinkTestData** (`0x06`, from whichever side is sending):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           Request ID          |       Filler (variable)       :
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+                               :
:                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Request ID (RID):** 2 bytes, u16. The LinkTest this belongs to.
- **Filler:** variable length, FS bytes of pseudo-random data (not all zeros: on byte streams COBS costs differ for zeros, random filler gives a realistic result).

The sender sends LinkTestData back to back, as fast as the link accepts, for the whole Duration. The packet Sequence numbers them, so the receiver can count losses.

**LinkTestResult** (`0x07`, D→C, the answer to LinkTest, sent when the test ends):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           Request ID          |         Messages Sent         |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|            (cont.)            |           Bytes Sent          |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|            (cont.)            |       Messages Received       |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|            (cont.)            |         Bytes Received        |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|            (cont.)            |            Elapsed            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|            (cont.)            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Request ID (RID):** 2 bytes, u16. The LinkTest it answers.
- **Messages Sent (MS):** 4 bytes, u32. LinkTestData messages the drone sent (0 when the client was sending).
- **Bytes Sent (BS):** 4 bytes, u32. Bytes the drone put on the link for them: whole frames on a byte stream, packets over UDP.
- **Messages Received (MR):** 4 bytes, u32. Valid LinkTestData messages the drone received (0 when the drone was sending).
- **Bytes Received (BR):** 4 bytes, u32. Bytes of those messages on the link, counted the same way as Bytes Sent.
- **Elapsed (El):** 4 bytes, u32. Drone's time in µs from the first to the last LinkTestData sent or received.

The client puts these next to its own counts to get bytes per second and the loss rate for the tested direction. Sticks keep flowing during a test, whichever side sends the test data, so the drone's link-loss logic is unaffected.

### 5.2 Control

**Sticks** (`0x10`, C→D):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|            Throttle           |              Roll             |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|             Pitch             |              Yaw              |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Throttle (Thr):** 2 bytes, u16. 0 … 65535, no centre.
- **Roll:** 2 bytes, i16. Left/right. −32768 … +32767, 0 = centred.
- **Pitch:** 2 bytes, i16. Forward/back. Same range.
- **Yaw:** 2 bytes, i16. Turn on the spot. Same range.

Values are stick positions, not distances; the flight code decides what they mean.

**The client sends Sticks at a steady rate whenever connected, even if nothing changed.** This is also the client's heartbeat. Target 50 Hz; may be lowered if the link is too slow (see §8).

### 5.3 Commands

**Arm** (`0x20`), **Disarm** (`0x21`), **Land** (`0x23`), **GoHome** (`0x24`), **SteadyState** (`0x26`), **ExitSteadyState** (`0x27`), all C→D:

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           Request ID          |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Request ID (RID):** 2 bytes, u16. See §5.0.

**TakeOff** (`0x22`, C→D):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           Request ID          |            Sent At            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|            (cont.)            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Request ID (RID):** 2 bytes, u16.
- **Sent At (SA):** 4 bytes, u32. Client's session clock in µs when the pilot gave the command. A resend keeps it.

**Flip** (`0x25`, C→D):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           Request ID          |            Sent At            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|            (cont.)            |           Direction           |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|            Duration           |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Request ID (RID):** 2 bytes, u16.
- **Sent At (SA):** 4 bytes, u32. Client's session clock in µs when the pilot gave the command. A resend keeps it.
- **Direction (Dir):** 2 bytes, u16. 0 = forward, increasing clockwise seen from above, one full circle over 0 … 65535.
- **Duration (Dur):** 2 bytes, u16. How long the flip takes, in ms (quick ≈ 300, slow ≈ 1500).

GoHome is reserved until GPS is fitted; until then the drone answers "not supported". Every command gets a Reply (§5.0).

**TakeOff and Flip expire.** Carried out late, after resends, they would surprise the pilot, perhaps close to the ground. The drone refuses one that is more than **300 ms** old with Reason 14 "too late". Age = the drone's session clock − Sent At, as a signed wrapping difference. The drone's clock runs behind the client's by the one-way delay, so a fresh command can look a few ms in the future: a negative age counts as fresh. The client also stops resending these two after 300 ms and tells the pilot. Every other command is carried out however late it arrives: a late Disarm or Land is still what the pilot wanted.

**SteadyState** asks the drone to hold still where it is (level, holding height; without GPS it may drift sideways) and, while it does, to accept only a small set of requests. **ExitSteadyState** returns to normal flying. The client keeps sending Sticks as its heartbeat; while in steady state the drone ignores their values. Which requests are allowed in steady state (e.g. LinkTest, GetSettings, RequestOnce, Subscribe, Land, Disarm, ExitSteadyState) is drone-side logic, to be decided later; other requests get a Reply "not allowed now" (§5.4).

### 5.4 Replies and events

**Reply** (`0x40`, D→C):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           Request ID          |  Answers Type |     Status    |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|             Reason            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Request ID (RID):** 2 bytes, u16. The request this answers.
- **Answers Type (AT):** 1 byte. The Type of that request.
- **Status (St):** 1 byte. One of: 0 = OK, 1 = refused, 2 = unknown type, 3 = malformed, 4 = not supported, 5 = busy, 6 = not allowed now (allowed in general, but not in the drone's current state, e.g. in steady state).
- **Reason (Rsn):** 2 bytes, u16. Only with Status refused, and then never 0: why the request was refused, from the table below. Each reason has **one number for all requests**, but each request may only use the reasons listed for it. Every other Status has Reason 0.

| Reason | Meaning |
|--------|---------|
| 1 | not armed |
| 2 | not flying |
| 3 | too low |
| 4 | battery low |
| 5 | throttle not low |
| 6 | IMU (motion sensor) not ready |
| 7 | arm switch off: the transmitter's arm switch is not in the armed position |
| 8 | unknown group |
| 9 | value out of range |
| 10 | unknown stream |
| 11 | rate not possible |
| 12 | unknown log |
| 13 | offset past the end |
| 14 | too late: the command is older than its expiry (§5.3) |

Reasons 8–13 belong to requests that `drone-common` doesn't implement yet; it adds a reason together with the first request that uses it.

What a Reply may say depends on the request:

| Request | Success | Refusal reasons |
|---------|---------|-----------------|
| Arm | Reply OK | 4 battery low, 5 throttle not low, 6 IMU not ready, 7 arm switch off |
| Disarm | Reply OK | never refused: disarming must always work |
| TakeOff | Reply OK | 1 not armed, 4 battery low, 14 too late |
| Land | Reply OK | 2 not flying |
| GoHome | Reply OK | none yet ("not supported" until GPS is fitted) |
| Flip | Reply OK | 1 not armed, 2 not flying, 3 too low, 14 too late |
| SteadyState | Reply OK | 2 not flying |
| ExitSteadyState | Reply OK | never refused |
| SetSettings | Reply OK | 8 unknown group, 9 value out of range |
| Subscribe | Reply OK | 10 unknown stream, 11 rate not possible |
| LinkTest | LinkTestResult, never Reply OK | none (e.g. while flying: Status "not allowed now") |
| GetSettings | Settings, never Reply OK | 8 unknown group |
| RequestOnce | StreamData, never Reply OK | 10 unknown stream |
| ListLogs | LogList, never Reply OK | none |
| ReadLog | LogChunk, never Reply OK | 12 unknown log, 13 offset past the end |

Arm is only accepted while the transmitter's arm switch is on (drone-design.md, "Failsafes"), so a client bug or a stray message can't start the motors on its own.

Rules:

- Status unknown type (2) only answers a Type the drone doesn't know. A known Type is never answered with it.
- Status malformed (3) answers a request that didn't decode (e.g. wrong length). A malformed message that isn't a request (e.g. Sticks) gets no Reply.
- For unknown type and malformed, the Request ID is the first 2 bytes of the payload. A payload shorter than that can't be answered and gets no Reply.
- Invalid: a Reason with any Status but refused; refused with Reason 0, with a Reason the request may not use (e.g. Flip refused with 5 throttle not low), or for a request that is never refused; OK to a request whose success is a data message; an Answers Type that is not a request (e.g. Sticks); malformed for a Type that isn't a known request.
- A Status or Reason the receiver doesn't know makes the Reply invalid (§2).

**Event** (`0x41`, D→C, never an answer, so no Request ID): the drone tells the client something happened.

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|            Event ID           |     Level     |      Code     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|    (cont.)    |                Data (variable)                :
+-+-+-+-+-+-+-+-+                                               :
:                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Event ID (EID):** 2 bytes, u16. The drone counts its events up (wrapping 65535 → 0). A resend keeps it, so the client shows each event once (§5.0a).
- **Level (L):** 1 byte. One of: 0 = debug, 1 = info, 2 = warning, 3 = error. Fixed per Code (table below); only a Text event chooses its own.
- **Code (C):** 2 bytes, u16. What happened, from the table below. 0 = Text.
- **Data:** variable length, 0 … 244 bytes, the rest of the payload; layout per Code. Most events have none.

Events (the high byte of the Code groups them):

| Code | Event | Level | Data |
|------|-------|-------|------|
| `0x0000` | Text | any | Text: 1 … 244 bytes of UTF-8. Where "any string from the drone" goes. |
| `0x0101` | Armed | info | none |
| `0x0102` | Disarmed | info | none |
| `0x0201` | Link lost | warning | none |
| `0x0202` | Link restored | info | none |
| `0x0301` | Battery low | warning | Millivolts: u16, whole pack |
| `0x0302` | Battery critical | error | Millivolts: u16, whole pack |
| `0x0401` | Failsafe triggered (an automatic safety action took over) | error | Cause: u8 (0 is never used). 1 = link loss, 2 = client frozen (the link is alive but the client stopped updating, §6 rule 5), 3 = low battery, 4 = loop overrun |
| `0x0402` | Loop overrun (the flight loop missed its deadline) | error | Late: u32, µs |
| `0x0403` | IMU error | error | none |

**Text** (Code `0x0000`):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|            Event ID           |     Level     |      Code     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|    (cont.)    |                Text (variable)                :
+-+-+-+-+-+-+-+-+                                               :
:                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Event ID (EID):** 2 bytes, u16.
- **Level (L):** 1 byte, any level.
- **Code (C):** 2 bytes, u16, 0.
- **Text:** variable length, 1 … 244 bytes of UTF-8.

**Battery low** (`0x0301`) and **Battery critical** (`0x0302`):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|            Event ID           |     Level     |      Code     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|    (cont.)    |           Millivolts          |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Event ID (EID):** 2 bytes, u16.
- **Level (L):** 1 byte. Warning for Battery low, error for Battery critical.
- **Code (C):** 2 bytes, u16. `0x0301` or `0x0302`.
- **Millivolts:** 2 bytes, u16. Voltage of the whole pack.

**Failsafe triggered** (`0x0401`):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|            Event ID           |     Level     |      Code     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|    (cont.)    |     Cause     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Event ID (EID):** 2 bytes, u16.
- **Level (L):** 1 byte, error.
- **Code (C):** 2 bytes, u16, `0x0401`.
- **Cause:** 1 byte. One of: 1 = link loss, 2 = client frozen, 3 = low battery, 4 = loop overrun. 0 is never used.

**Loop overrun** (`0x0402`):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|            Event ID           |     Level     |      Code     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|    (cont.)    |                      Late                     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|    (cont.)    |
+-+-+-+-+-+-+-+-+
```

where:

- **Event ID (EID):** 2 bytes, u16.
- **Level (L):** 1 byte, error.
- **Code (C):** 2 bytes, u16, `0x0402`.
- **Late:** 4 bytes, u32, in µs. How late the loop run finished.

Rules:

- A known Code with a different Level, or with Data that doesn't match its layout, makes the Event invalid.
- A Text event whose text is empty or not valid UTF-8 is invalid.
- A Code, Level or Cause the receiver doesn't know makes the Event invalid (§2). New events are added to this table and to `drone-common` together, in a new protocol version.

### 5.5 Settings

Settings are organised in **groups**. Each group has a 1-byte ID (up to 255 groups; if that ever runs out, a new protocol version widens it) and a **fixed layout of values defined once in `drone-common`**, known to both sides. Layouts can branch: e.g. the estimator group starts with "which filter" and the rest depends on that choice. The layout is not described on the wire; the version handshake guarantees both sides agree.

A **Setting Group** (used inside SetSettings and Settings):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|    Group ID   |     Length    |       Values (variable)       :
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+                               :
:                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Group ID (GID):** 1 byte, u8. 0 is never used.
- **Length (Len):** 1 byte. Number of bytes in Values; Len == size(Values). Lets the receiver reject malformed messages cleanly.
- **Values:** Len bytes, layout per group. Fractional values are f32.

**SetSettings** (`0x50`, C→D):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           Request ID          | Setting Group list (variable) :
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+                               :
:                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Request ID (RID):** 2 bytes, u16.
- **Setting Group list:** variable length, one or more [Setting Group] back to back, until the end of the payload.

**SetSettings is all-or-nothing.** The drone checks every group first. Any problem → nothing changes, one Reply with the reason. All good → every change takes effect at the same moment, between two flight-loop runs, and one Reply with OK.

**GetSettings** (`0x51`, C→D):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           Request ID          |    Group ID list (variable)   :
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+                               :
:                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Request ID (RID):** 2 bytes, u16.
- **Group ID list:** variable length, one or more u8 group IDs.

**Settings** (`0x52`, D→C, the answer to GetSettings):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           Request ID          | Setting Group list (variable) :
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+                               :
:                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Request ID (RID):** 2 bytes, u16. The GetSettings it answers.
- **Setting Group list:** variable length, one [Setting Group] per requested group ID, in the requested order.

An unknown group ID gives a Reply instead.

First groups (layouts defined in `drone-common`): PID gains (per axis), attitude estimator (filter choice + that filter's values).

### 5.6 Data streams

Each kind of live data is a numbered **stream** (1-byte ID, 0 never used; up to 255 streams).

**Subscribe** (`0x61`, C→D):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           Request ID          |   Stream ID   |      Rate     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|    (cont.)    |
+-+-+-+-+-+-+-+-+
```

where:

- **Request ID (RID):** 2 bytes, u16.
- **Stream ID (SID):** 1 byte, u8.
- **Rate:** 2 bytes, u16, in Hz. 0 = stop.

Answered with a Reply (OK, or e.g. unknown stream / rate not possible).

**RequestOnce** (`0x60`, C→D):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           Request ID          |   Stream ID   |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Request ID (RID):** 2 bytes, u16.
- **Stream ID (SID):** 1 byte, u8.

Answered with one StreamData carrying this Request ID (or a Reply on error).

**StreamData** (`0x62`, D→C):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           Request ID          |   Stream ID   |               :
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+               :
:                        Data (variable)                        |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Request ID (RID):** 2 bytes, u16. The RequestOnce it answers.
- **Stream ID (SID):** 1 byte, u8.
- **Data:** variable length, layout per stream.

**StreamUpdate** (`0x63`, D→C): stream data nobody asked for right now: the default set and subscribed streams. Same as StreamData, without the Request ID.

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|   Stream ID   |                 Data (variable)               :
+-+-+-+-+-+-+-+-+                                               :
:                                                               |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Stream ID (SID):** 1 byte, u8.
- **Data:** variable length, layout per stream (the same as in StreamData).

Rules:

- After a successful handshake, the drone starts a **default set** (the glance screen) without being asked, sent as StreamUpdate: status, battery, attitude, height, link quality, at a few Hz. Exact rates: to be decided.
- Streams can be subscribed, changed and stopped during flight.
- Stream data uses **fixed-point whole numbers** (exact, small), e.g. mV, centidegrees, cm.

First streams (field layouts defined in `drone-common`, units as given):

| ID | Stream | Units |
|----|--------|-------|
| 1 | Status: armed, mode, warning flags | — |
| 2 | Battery | mV |
| 3 | Attitude (roll, pitch, yaw) | 0.01° |
| 4 | Height | cm |
| 5 | Raw IMU (accel, gyro) | sensor units, documented per field |
| 6 | GPS (reserved, later) | 1e-7° lat/lon |
| 7 | Link quality (good/dropped frame counts) | — |

### 5.7 Logs

The drone records detailed data (sensors, PID internals, motor outputs) to its SD card. The client can fetch it through the protocol: slowly over radio, quickly over WiFi.

**ListLogs** (`0x70`, C→D):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           Request ID          |  First Log ID |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Request ID (RID):** 2 bytes, u16.
- **First Log ID:** 1 byte, u8. List logs from this Log ID up; 0 lists from the start.

**LogList** (`0x71`, D→C):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           Request ID          |  Next Log ID  |               :
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+               :
:                   Log Entry list (variable)                   |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Request ID (RID):** 2 bytes, u16. The ListLogs it answers.
- **Next Log ID:** 1 byte, u8. The First Log ID to ask for the next page; 0 when this is the last page.
- **Log Entry list:** variable length, 0 to 49 [Log Entry] (what fits in 255), sorted by Log ID, starting at the first log whose Log ID ≥ First Log ID.

A **Log Entry**:

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|     Log ID    |                      Size                     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|    (cont.)    |
+-+-+-+-+-+-+-+-+
```

where:

- **Log ID (LID):** 1 byte, u8.
- **Size:** 4 bytes, u32, in bytes.

**ReadLog** (`0x72`, C→D):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           Request ID          |     Log ID    |     Offset    |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                    (cont.)                    |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Request ID (RID):** 2 bytes, u16.
- **Log ID (LID):** 1 byte, u8.
- **Offset (Off):** 4 bytes, u32. Byte position to start reading from.

**LogChunk** (`0x73`, D→C):

```
 0               1               2               3
 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7 0 1 2 3 4 5 6 7
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|           Request ID          |     Log ID    |     Offset    |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                    (cont.)                    |               :
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+               :
:                        Data (variable)                        |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

where:

- **Request ID (RID):** 2 bytes, u16. The ReadLog it answers.
- **Log ID (LID):** 1 byte, u8.
- **Offset (Off):** 4 bytes, u32. Same as requested.
- **Data:** variable length, up to 242 bytes (what fits in 255).

Rules:

- **Log IDs are 1 byte**, so the drone lists at most 255 logs (the newest ones; 0 is never used). Older logs stay on the SD card and can be fetched over WiFi as files.
- The client asks by **position**, so a lost chunk or page is just asked for again; the drone keeps no download state.
- **Listing more than one page:** if Next Log ID is not 0, the client sends a new ListLogs (new Request ID) with that value as First Log ID. Each ListLogs gets exactly one LogList, so pages of two lists in flight never mix: each carries its own Request ID.
- An Offset exactly at the end returns a LogChunk with no Data (the end of the log); an Offset past the end is refused with Reason 13.
- What goes into a log and its file format: out of scope here.

## 6. Behaviour rules the protocol relies on

1. Only messages with a valid CRC count, for anything.
2. Each side's sequence counter starts at 0 at handshake and increments per sent message.
3. A Reply refers to the request's Request ID and type.
4. **Liveness:** the client's Sticks stream is its heartbeat; the drone's default status stream is the drone's heartbeat. How long a silence means "link lost" is drone/client logic (the design doc's 250 ms).
5. **Liveness over CRSF:** in standard ELRS mode the TX module keeps repeating the last values even if the client program freezes, so silence is not enough. As already in drone-design.md, the client puts a counter on a spare channel that changes every frame. The CRSF adapter's link health records both "last valid frame" and "last time the counter changed"; the internal state's link-health type has room for both. Which channel: to be decided.

## 7. Code structure

Two shared crates, both `no_std` (no operating system or heap needed) so the flight side can use them safely.

**`drone-common`: the language both sides speak.** Types and bytes, no behaviour.

```
common/src/
  domain.rs    internal state: Sticks, ArmState, ArmSwitch, LinkHealth (with Uptime, this side's own clock),
               RcChannels (raw CRSF channel values); later Command, flight mode, settings values
  wire/        our protocol
    crc16.rs      (private)
    cobs.rs       (private)
    packet.rs     Packet: checked Flags / Type / Sequence / Payload / (Auth Tag) / CRC; PacketError
    stream.rs     COBS framing for byte streams: stream::encode, StreamDecoder
    message.rs    the list of messages: each message's struct, type byte and (for requests) refusal codes, written once;
                  enum Message { Hello(..), Sticks(..), Arm(..), ... } + encode / decode; Answer; Request; MessageError
    reply.rs      building a Reply (its struct is in the message list): Reply::to(&request, outcome),
                  Reply::to_invalid(&packet) for "malformed" / "unknown type"; Outcome; the shared Reason table
    event.rs      the list of events (Armed, BatteryLow { millivolts }, ...) with code and level, written once, and
                  EventKind over them; EventId; the Event message (ID + EventKind) is in message.rs' list;
                  Text with EventText, a string that needs no heap
    payload.rs    (private) Wire: how each field reads and writes itself; Code: enums stored as numbers
    macros/       (private) code!, wire_struct!, messages!, events!: turn those lists into types
  crsf.rs      CRC-8 and CRSF constants; later frame parsing, CRSF channels → Sticks
```

- The **internal state** types the adapters produce (sticks, pending command, settings changes, link health). They are protocol-neutral: CRSF units (172–1811) only exist in `RcChannels` and `crsf.rs`.
- A Rust type for every message in §5 with encode/decode. Messages are shaped like the wire; where a message *is* a domain concept (Sticks), there is one type: it is defined with the messages and `domain` re-exports it.
- Three layers, each with its own type, so the compiler keeps them apart:
  - `Message::encode(sequence, buf) -> Packet` and `Message::decode(&Packet) -> Result<Message, MessageError>`.
  - `Packet::parse(bytes) -> Result<Packet, PacketError>` checks length, CRC and flags. A `Packet` can only exist after those checks, so `Message::decode` never sees unchecked bytes. UDP uses this directly.
  - `stream::encode(&Packet, buf)` and `StreamDecoder::next_packet(&mut chunk)` add and remove COBS on byte streams.
  - CRC-16 and COBS themselves are private. The sequence counter belongs to the caller.
  - `PacketError` (damaged or not for us) is dropped silently; `MessageError` (intact but invalid) can be answered, and the caller still holds the packet to do so.
- Settings group and stream layouts (once those messages exist); Reason and event codes.
- Tests: round-trip every message; corrupted/truncated packets are rejected; COBS edge cases (zeros, 254-byte runs, max size); streams split at every chunk size.

**`drone-link`: behaviour both sides need.** Depends on `drone-common`, never the other way around (Rust forbids circular dependencies between crates, so the boundary enforces itself). Created when its first piece is needed.

- Sequence-gap counting, the delivery bookkeeping of §5.0a (what to acknowledge, what is lost, the RTT estimate; the caller passes in the time), session-clock helpers, a generic "remember the last N IDs" (Request IDs, Event IDs), the auth tag computation (later).

**Stays in `fc` / `client`:** deciding whether to obey a request, the clocks and timers that drive `drone-link`, link-loss timeouts, the handshake state machine, and all I/O (serial ports, sockets, clocks).

### 7.1 Decisions

- **Decoded messages own their data.** Variable-length fields (Event text, LogChunk data, setting group lists, …) are copied out of the receive buffer into a fixed-size array plus a length inside the message. Every message is therefore as big as the biggest one (~250 bytes), which is fine; in return messages can be kept, queued and handed to other threads without Rust lifetimes everywhere. Switching to borrowing from the receive buffer stays possible if copying ever matters.
- **Decoded messages are typed, not raw bytes.** Codes are enums and bit fields are structs of named `bool`s; plain numbers stay numbers (versions, counts, µs, cm). A code enum holds only the values the protocol defines: a number it doesn't know fails decoding (`MessageError::UnknownCode`, with the number, so it can be logged), because a peer that passed Hello knows the same codes. So every value that can be built can also be sent. The two exceptions hold only what the other side really didn't know: `UnknownType`, a type byte this build has no message for (to answer "unknown type"), and `UnknownOptions`, Hello option bits without a meaning yet. Combinations the protocol forbids (e.g. a Reply OK with a Reason) are made impossible by the types, and rejected when decoding.
- **Rust types follow the implemented messages.** A message type from §5 that has no Rust message yet (e.g. LinkTest) is unknown to that build, and its codes are added together with it.
- **Three kinds of checks:**
  1. **Incoming data** (known enum values, ranges, lengths): part of decoding, **always on, also in release builds**, because release is the build that flies. A decoded message is always a valid message.
  2. **Our own assumptions** ("this can never happen", e.g. the encoder's payload fits in 249 bytes): `debug_assert!`, debug builds only.
  3. **Extra logging:** a runtime switch (e.g. a verbose flag), so it can be turned on in the build that flies.

  Debug builds may *add* logs and asserts but never *decide* differently from release builds.
- **Wrapping arithmetic is explicit.** Sequence numbers, Request IDs and session-clock times use `wrapping_add` / `wrapping_sub`. Plain `+` panics on overflow in debug builds but silently wraps in release, which would make the two behave differently.

## 8. Open risks

- **Airport throughput is unknown.** 50 Hz Sticks ≈ 16 B × 50 = 800 B/s plus streams, plus 16 B per Ack (only sent when a reliable message arrived). Measure on the bench with LinkTest (§5.1a), both directions. Fallbacks: send Sticks at 25 Hz, fewer streams in flight, 12-bit axes in a new version.

- **Possible later: separate radio and WiFi modes**, chosen in the handshake (an option bit), so WiFi can use bigger messages and fields (e.g. message size, Request ID width) while radio stays small. Not now: one format for all links.

## 9. Still to decide

- CRSF channel → internal state mapping (which AUX channel is arm, flip, etc.).
- Exact stream field layouts and settings group layouts beyond what is listed.
- Default stream rates. The 20 ms ack delay, 300 ms expiry and 100 ms starting RTT are first guesses, to be measured.
