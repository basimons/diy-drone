# DIY Raspberry Pi Drone — Design

*Started 2026-10-19*

## 1. Intent

A 5-inch quadcopter, built for fun and for learning, where a **single Raspberry Pi 5 is both the flight controller and the higher-level computer**. The flight code is written from scratch (that is the point).

**Why one Pi:** fewer parts, and full ownership of the flight code.

**Approach:** run the stabilization loop on a PREEMPT_RT kernel with a pinned, isolated CPU core. If the timing turns out not to be good enough, fall back to a separate flight-controller board. Parts are chosen so that fallback is cheap (standard ESC protocols, room on the frame for a small FC).

### Goals (first build)
- Stable, controllable hover and gentle flight on the RC transmitter.
- Pi decodes the RC link itself (CRSF serial from an ELRS receiver).
- Failsafes that make crashes boring rather than dangerous.

### Non-goals (first build)
- GPS, position hold, autonomous missions.
- Camera, image processing, FPV video.
- Acro/freestyle performance.

### Anti-goals (failure even if it "works")
- Motors that can spin up unexpectedly (on boot, on code crash, on link loss).
- Flying near people or property before the failsafes are proven.

### Later (planned, not bought yet)
- GPS (u-blox M10 + compass, second UART).
- Pi Camera Module 3 Wide, image work on the free cores.
- Writing the raw RF protocol on the Pi (SX1280 over SPI) instead of using the ELRS receiver's decoded output.
- DShot to the ESC via the Pi 5's RP1 PIO.

## 2. Parts list (first order)

| # | Part | Spec to look for | Where | ~€ |
|---|------|------------------|-------|----|
| 1 | Raspberry Pi 5 | ✅ **1 GB** (2026 RAM prices make 4/8 GB €150+) + official Active Cooler + 32 GB A2 microSD. 1 GB is fine for headless OS + flight code; compile on the laptop (cross-compile) or add swap | RaspberryStore.nl (approved reseller) | 66 + SD |
| 2 | Frame | ✅ **Mark4 5" (225 mm, 5 mm arms), "Mark4 5IN" variant**. Pi sits on 10–15 mm standoffs above top plate (overhangs; must be above prop plane) | Ali | 22 |
| 3 | Motors ×4 | ✅ **Samguk Series Wei 2207 2300KV, 4pcs** (gentler choice for a ~540 g drone on 4S) | Ali | 39 |
| 4 | 4-in-1 ESC | ✅ **YSIDO BLS-45A 4in1 (ESC only, not the FC option)**, BLHeli_S, 30.5 mm, XT60 + capacitor included. Not Bluejay; reflash if it arrives with Bluejay | Ali | 36 |
| 5 | Props | 5" tri-blade, 3–4 packs | Ali | 8 |
| 6 | IMU | ✅ **LSM6DSV + QMC6309 module (SlimeVR style), 1pcs**. SPI pins + INT1/INT2; compass bonus. Check VCC pin/voltage on arrival | Ali | 8 |
| 7 | Barometer | ✅ **ICGOICIC BMP390 module**, I2C | Ali | 6 |
| 8 | Power monitor | ✅ **TENSTAR INA226 (CJMCU-226), 2 pcs**. Used for **voltage only** via the VBS pin (onboard shunt is only good to ~1 A). Current sensing later via the ESC current pad | Ali | 4 |
| 12 | Charger | ✅ **Haisito/HTRC C150** (AC/DC, 150 W, LiPo bag included) | Amazon.fr | 60 |
| 13 | Batteries ×2 | ✅ **OVONIC 4S 1550 mAh 100C, XT60** | Amazon.fr | 40 |
| 9 | BEC | ✅ **Generic UBEC, 7A option** (5.5–35 V in, 6 A continuous, 5.25 V out, 18 g). Measure output before connecting the Pi | Ali | 6 |
| 10+11 | RC link | ✅ **Jumper "ELRS TX RX" set**: AION Nano T-Pro TX module (5–8.4 V in) + AION RX Mini receiver (3.5–6 V). **No handheld:** PS5 controller → laptop (later Android phone) → USB-serial adapter → TX module | Ali | 49 |
| 11a | USB-serial adapter | ✅ **FT232RL USB-TTL (Type-C), jumper set to 3.3 V**. Chosen over CP2102 because it hits CRSF's 400 000 baud exactly. Half-duplex wiring: adapter TX → 1 kΩ → module S.port; adapter RX → module S.port directly; GND common. Software discards the echo | Ali | 5 |
| 11d | Resistor kit | ✅ 1/4 W metal film, 30 values (300 pcs, box). Includes 1 kΩ | Ali | 4.5 |
| 11b | TX module power | ✅ **MT3608 Type-C step-up, 3 pcs**, set to ~7.4 V with a multimeter *before* connecting. Use a USB-A→C cable | Ali | 2 |
| 11c | Multimeter | ✅ **ANENG SZ304** | Ali | 11 |
| 5 | Props (bought) | ✅ **16 pcs (8 pairs) 5150 tri-blade, PC, 5 mm hole** | Ali | 7 |
| 14 | Misc | ✅ YSIDO ShortSaver 2 smoke stopper · M2.5 nylon standoff kit (Pi holes are M2.5) · 120 pcs Dupont jumper kit · 22 AWG silicone wire red+black 5 m · heat-shrink kit · 3 mm foam pads (IMU soft-mount). LiPo bag comes with the charger | Ali | 22 |
| 15 | Soldering | ✅ JCD 908S 80 W iron kit (EU) · rosin flux paste · 63/37 0.8 mm solder (50 g) | Ali | 20 |

**Total ≈ €375** (drone itself ≈ €220; transmitter/charger/batteries are reusable).

Also needed if not owned: soldering iron + solder, heat-shrink, multimeter.

**Weight estimate:** frame 110 + motors 130 + ESC 12 + Pi/cooler 55 + sensors/RX/BEC 20 + wiring/mounts 30 + battery 180 ≈ **540 g**. Fine for 5" on 4S.

## 3. Wiring (Pi 5 header)

| Function | Bus | Pi GPIO (header pin) |
|----------|-----|----------------------|
| Motor 1–4 | Hardware PWM (RP1 PWM0 ch 0–3) | GPIO12 (32), GPIO13 (33), GPIO18 (12), GPIO19 (35) |
| IMU | SPI0 | MOSI GPIO10 (19), MISO GPIO9 (21), SCLK GPIO11 (23), CS GPIO8 (24) |
| IMU data-ready | GPIO interrupt | GPIO25 (22) |
| Baro + INA226 | I2C1 (3.3 V) | SDA GPIO2 (3), SCL GPIO3 (5) |
| ELRS receiver | UART0, CRSF 420 000 baud | TX GPIO14 (8) → RX on receiver, RX GPIO15 (10) ← TX on receiver |
| GPS (later) | Another UART | e.g. GPIO4/5. Verify the overlay. |
| Power in | BEC 5 V | 5 V pins (2, 4), GND (6) |

Notes:
- All Pi I/O is **3.3 V**. The IMU, baro, INA226 and receiver must run at 3.3 V logic. ESC signal inputs accept 3.3 V.
- ESC signal ground goes to Pi GND.
- Powering through the 5 V header pins bypasses the Pi's input protection. Use a good BEC, and set `usb_max_current_enable=1` in `config.txt` (otherwise the Pi 5 assumes a weak supply).
- Driving all four PWM channels needs a device-tree overlay. The stock `pwm-2chan` covers only two, so a custom overlay is probably required. **Verify on the bench first.**
- Disable the serial console on UART0.
- Soft-mount the IMU (grommets/foam). Vibration is the #1 enemy of a homemade FC.

## 4. Software architecture

**OS:** Raspberry Pi OS Lite 64-bit with a PREEMPT_RT kernel.
Boot args: `isolcpus=3 nohz_full=3 rcu_nocbs=3 irqaffinity=0-2`, CPU governor `performance`.

| Core | Job | Rate |
|------|-----|------|
| 3 (isolated) | **Flight loop**: read IMU → attitude estimate → PID → motor mix → PWM | 1 kHz to start (target 2–4 kHz later) |
| 2 | RC (CRSF parse), barometer, battery monitor. Hands values to core 3 via lock-free shared memory. | 50–500 Hz |
| 0–1 | Linux, logging, WiFi telemetry/tuning UI, later GPS/camera | — |

Flight loop rules: `SCHED_FIFO` high priority, `mlockall`, no allocation/syscalls/logging inside the loop. Written in **C, C++ or Rust**. Python is fine for tooling but not for the loop. *Language: user's choice, still open.*

**ESC protocol:** OneShot125 via hardware PWM first. Later, DShot via RP1 PIO.

**Flight modes:** rate (gyro) loop first, then angle (self-level) mode on top for easy flying.

### Failsafes (built before the first prop goes on)
- **Arming:** only while the transmitter's arm switch is on, with throttle low and a healthy IMU. The client can also send Arm, but it is refused unless the switch is on. Disarmed on boot.
- **RC link loss:** no valid CRSF frame for >250 ms → disarm (later: controlled descent).
- **Laptop program freeze/crash:** the TX module may keep sending stale values. The laptop program sends a counter on a spare channel that increments every frame; if it stops changing for >250 ms, the Pi treats it as link loss.
- **Gamepad disconnect:** laptop program detects controller loss and sends disarm.
- **Loop overrun:** flight loop misses deadlines or stalls → a watchdog on another core forces the motors to zero.
- **Process crash/exit:** the PWM must fall to "motor off". Verify on the bench by killing the process.
- **Low battery:** warn, then land/disarm.

## 5. Bring-up milestones

1. Pi + RT kernel: measure jitter with `cyclictest` on the isolated core.
2. IMU over SPI at 1 kHz+, check noise with the motors off and then on (props off).
3. CRSF decode from the receiver, values shown live.
4. Motors spin individually from code (**props off**). Check direction and order.
5. Failsafe tests, all props off.
6. Tethered/held hover test outdoors.
7. Free hover in an open field, away from people.

## 6. Legal (EU/CH)

At ~540 g this is above 250 g. You need **operator registration** with your national aviation authority (EASA rules, also adopted by Switzerland) and the free online **A1/A3 training**. Fly in the Open category, subcategory A3: far from people and buildings.

## 7. Open questions
- Programming language for the flight code.
- Exact frame model (must fit the Pi on top; maybe a 3D-printed mount).
