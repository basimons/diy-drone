# Link Protocol — Design

*2026-10-22. Status: agreed in conversation, ready for planning.*

## 1. Intent

The drone and the laptop talk over a byte link. We design **our own protocol** for that link, for fun and for learning, instead of being limited to what ELRS's CRSF format can carry.

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
- **Backwards compatibility.** Breaking older code on both sides is fine; that is what the protocol version is for.
- Authentication/signing itself (only the room for it).
- Deciding *when the drone accepts or refuses* something (e.g. "settings only while disarmed", "no flip below X m"). That is drone-side logic, built later. The protocol only provides the way to say "refused, because…".
- Encryption (hiding content). Nothing sent is secret.

**Anti-goals**
- A format where adding a message or setting forces a redesign.
- A stale or frozen laptop that the drone cannot detect (see §6 rule 4).

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
- Over CRSF, simple commands can be mapped to switch channels (e.g. "channel 8 high" → flip). The exact mapping is left to the builder (§9).

## 4. Framing

### 4.1 Envelope

Every message, before framing:

```
[flags 1B] [type 1B] [sequence 2B] [payload ...] [auth tag, later] [CRC-16 2B]
```

| Field | Size | Meaning |
|-------|------|---------|
| flags | 1 B | bit 0 = "auth tag present". Bits 1–7 reserved, must be 0 for now. Until authentication exists, a receiver drops any frame with a non-zero flags byte. *(My call.)* |
| type | 1 B | Message type, see §5. 256 values; more needed → new protocol version. |
| sequence | 2 B | Counter per direction, +1 per message sent (resends too), wraps 65535 → 0. Used for loss detection, frozen-laptop detection, and later replay protection. Matching replies to requests uses the separate request ID (§5.0). |
| payload | 0+ B | Depends on type. |
| auth tag | 0 B now | Only present when flags bit 0 is set. Not defined yet. |
| CRC-16 | 2 B | Over everything before it (flags through auth tag). |

- **All multi-byte numbers are little-endian.**
- **Maximum message size: 255 bytes** before COBS encoding (a deliberate choice: fixed buffers, small radio packets; raise in a later version if needed).
- **CRC variant:** CRC-16/CCITT-FALSE (polynomial 0x1021, init 0xFFFF, no reflection, no final XOR). *(My call.)*

### 4.2 COBS

The envelope is COBS-encoded (Consistent Overhead Byte Stuffing) so it contains no `0x00` bytes, then followed by a single `0x00` delimiter.

- A receiver that loses sync just waits for the next `0x00`.
- Message length is known from the delimiter position, so there is no length field.
- Messages ≤ 254 bytes cost exactly 1 extra byte, plus the delimiter.

**Receive rule:** decode the frame, check length ≤ 255, check CRC. Anything that fails is dropped silently (counted for link-quality stats).

## 5. Message types

`0x00` is never used. Ranges group topics; `0x80–0xFF` are free.

| Range | Topic | Types |
|-------|-------|-------|
| `0x01–0x0F` | Connection | `0x01` Hello, `0x02` HelloReply, `0x03` Ping, `0x04` Pong |
| `0x10–0x1F` | Control | `0x10` Sticks |
| `0x20–0x3F` | Commands | `0x20` Arm, `0x21` Disarm, `0x22` TakeOff, `0x23` Land, `0x24` GoHome, `0x25` Flip |
| `0x40–0x4F` | Replies & events | `0x40` Reply, `0x41` Event |
| `0x50–0x5F` | Settings | `0x50` SetSettings, `0x51` GetSettings, `0x52` Settings |
| `0x60–0x6F` | Data streams | `0x60` RequestOnce, `0x61` Subscribe, `0x62` StreamData |
| `0x70–0x7F` | Logs | `0x70` ListLogs, `0x71` LogList, `0x72` ReadLog, `0x73` LogChunk |

Direction: L→D = laptop to drone, D→L = drone to laptop.

### 5.0 Requests and answers *(my call)*

- A **request** is any L→D message except Sticks and Ping: commands, SetSettings, GetSettings, Subscribe, RequestOnce, ListLogs, ReadLog.
- Every request payload **starts with a request ID (u16)**, chosen by the laptop, before the fields listed below.
- Every request gets **exactly one answer**: its data message (Settings, StreamData, LogList, LogChunk, which echo the request ID first), or a Reply. Errors (unknown stream/group/log ID, bad rate, refused) are always a Reply.
- **Resending:** if no answer arrives within a timeout, the laptop resends the request with the **same request ID** (but a new sequence number). The drone remembers the last 16 request IDs it handled; a repeat is answered again from that memory without executing twice.
- Unsolicited data (default streams, subscribed streams, Events) uses request ID 0. The laptop never uses 0 for a request.

### 5.1 Connection

| Type | Dir | Payload |
|------|-----|---------|
| Hello | L→D | protocol version u16, wanted options u32 |
| HelloReply | D→L | protocol version u16, result u8, accepted options u32 |
| Ping | either | timestamp u32 (sender's clock, ms) |
| Pong | either | the timestamp from the Ping, echoed |

- **Protocol version** starts at `1`.
- **Options** are 32 on/off bits. Bit 0 = authentication. Bits 1–31 free.
- **HelloReply result:** 0 = OK, 1 = version mismatch, 2 = option not supported.
- **Until a handshake succeeds, everything except Hello and Ping is ignored.**
- **Hello and HelloReply carry sequence 0.** On a valid Hello the drone resets both counters; its HelloReply is its message 0. *(My call.)*
- **A new Hello restarts the session at the protocol layer only:** sequence counters, subscriptions (back to the default set) and the remembered request IDs. It does **not** touch the internal state (sticks, armed state, settings). Sticks stop arriving while the laptop reconnects; that gap is handled by the normal link-loss logic like any other silence. *(My call.)*
- When authentication is added, the handshake is also where both sides exchange fresh random numbers for the session (protection against replaying recorded messages). Not now.

### 5.2 Control

| Type | Dir | Payload |
|------|-----|---------|
| Sticks | L→D | throttle u16, roll i16, pitch i16, yaw i16 (8 B) |

- Throttle 0 … 65535 (no centre). Roll/pitch/yaw −32768 … +32767, 0 = centred.
- Roll = left/right, pitch = forward/back, yaw = turn on the spot.
- Values are stick positions, not distances; the flight code decides what they mean.
- **The laptop sends Sticks at a steady rate whenever connected, even if nothing changed.** This is also the laptop's heartbeat. Target 50 Hz; may be lowered if the link is too slow (see §8).

### 5.3 Commands

| Type | Dir | Payload |
|------|-----|---------|
| Arm, Disarm, TakeOff, Land, GoHome | L→D | none |
| Flip | L→D | direction u16, duration_ms u16 |

- **Flip direction:** 0 = forward, increasing clockwise seen from above, one full circle over 0 … 65535.
- **Flip duration:** how long the flip takes, in ms (quick ≈ 300, slow ≈ 1500).
- GoHome is reserved until GPS is fitted; until then the drone answers "not supported".
- **Every command gets a Reply.** If the laptop gets none within a timeout it resends; the drone recognises a repeat by its sequence number and replies again without executing twice. *(Timeout value: builder's call.)*

### 5.4 Replies and events

| Type | Dir | Payload |
|------|-----|---------|
| Reply | D→L | request ID u16, answers type u8, status u8, reason u16 |
| Event | D→L | level u8, code u16, text (UTF-8, rest of payload) |

- **Reply status:** 0 = OK, 1 = refused, 2 = unknown type, 3 = malformed, 4 = not supported, 5 = busy.
- **Reason:** code whose meaning depends on the command (e.g. "too low", "battery low"). 0 = no reason given. Code list: builder's call, defined in `drone-common`.
- **Event level:** 0 = debug, 1 = info, 2 = warning, 3 = error. Free text is where "any string from the drone" goes.

### 5.5 Settings

Settings are organised in **groups**. Each group has a 16-bit ID and a **fixed layout of values defined once in `drone-common`**, known to both sides. Layouts can branch: e.g. the estimator group starts with "which filter" and the rest depends on that choice. The layout is not described on the wire; the version handshake guarantees both sides agree.

| Type | Dir | Payload |
|------|-----|---------|
| SetSettings | L→D | list of `[group ID u16][length u8][values]` |
| GetSettings | L→D | list of group ID u16 |
| Settings | D→L | list of `[group ID u16][length u8][values]` |

- **SetSettings is all-or-nothing.** The drone checks every group first. Any problem → nothing changes, one Reply with the reason. All good → every change takes effect at the same moment, between two flight-loop runs. One Reply with OK.
- The per-group length lets the receiver reject malformed messages cleanly.
- Decimal values in settings are **f32** (4 B).
- First groups (layouts: builder's call): PID gains (per axis), attitude estimator (filter choice + that filter's values).

### 5.6 Data streams

Each kind of live data is a numbered **stream** (16-bit ID).

| Type | Dir | Payload |
|------|-----|---------|
| Subscribe | L→D | stream ID u16, rate Hz u16 (0 = stop) |
| RequestOnce | L→D | stream ID u16 |
| StreamData | D→L | stream ID u16, stream data |

- After a successful handshake, the drone starts a **default set** (the glance screen) without being asked: status, battery, attitude, height, link quality, at a few Hz. *(Exact rates: builder's call.)*
- Streams can be subscribed, changed and stopped during flight.
- Stream data uses **fixed-point whole numbers** (exact, small), e.g. mV, centidegrees, cm.

First streams (field layouts: builder's call, units as given):

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

The drone records detailed data (sensors, PID internals, motor outputs) to its SD card. The laptop can fetch it through the protocol: slowly over radio, quickly over WiFi.

| Type | Dir | Payload |
|------|-----|---------|
| ListLogs | L→D | none |
| LogList | D→L | list of `[log ID u16][size u32]` |
| ReadLog | L→D | log ID u16, start offset u32 |
| LogChunk | D→L | log ID u16, offset u32, data (up to what fits in 255 B) |

- The laptop asks by **position**, so a lost chunk is just asked for again; the drone keeps no download state.
- An offset at/after the end returns a LogChunk with no data.
- What goes into a log and its file format: out of scope here.

## 6. Behaviour rules the protocol relies on

1. Only messages with a valid CRC count, for anything.
2. Each side's sequence counter starts at 0 at handshake and increments per sent message.
3. A Reply refers to the request's request ID and type.
4. **Liveness:** the laptop's Sticks stream is its heartbeat; the drone's default status stream is the drone's heartbeat. How long a silence means "link lost" is drone/client logic (the design doc's 250 ms).
5. **Liveness over CRSF:** in standard ELRS mode the TX module keeps repeating the last values even if the laptop program freezes, so silence is not enough. As already in drone-design.md, the client puts a counter on a spare channel that changes every frame. The CRSF adapter's link health records both "last valid frame" and "last time the counter changed"; the internal state's link-health type has room for both. *(Which channel: builder's call.)*

## 7. What goes in `drone-common`

- Envelope encode/decode, COBS, CRC-16 — `no_std`, no allocation, fixed 255-byte buffers.
- A Rust type for every message in §5 with encode/decode.
- Settings group and stream layouts.
- The **internal state** types the adapters produce (sticks, pending command, settings changes, link health).
- The CRSF adapter's frame parsing (CRSF RC channels and the standard telemetry frames).
- Tests: round-trip every message; corrupted/truncated frames are rejected; COBS edge cases (zeros, 254-byte runs, max size).

## 8. Open risks

- **Airport throughput is unknown.** 50 Hz Sticks ≈ 16 B × 50 = 800 B/s plus streams. Measure on the bench. Fallbacks: send Sticks at 25 Hz, fewer streams in flight, 12-bit axes in a new version.

## 9. Left to the builder

- CRSF channel → internal state mapping (which AUX channel is arm, flip, etc.).
- Exact stream field layouts and settings group layouts beyond what is listed.
- Reply reason codes and event codes.
- Command resend timeout and default stream rates.
- Module/file structure inside `drone-common`.
