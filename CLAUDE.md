# CLAUDE.md

A 5" quadcopter where a Raspberry Pi 5 runs hand-written flight code in Rust, plus a ground client.
Read [README.md](README.md) for the crates and commands, [drone-design.md](drone-design.md) for
hardware and goals, and [protocol.md](protocol.md) for the client ↔ drone link protocol.

## Commands

```sh
scripts/check.sh                          # everything CI runs: fmt check, clippy (warnings = errors), tests
cargo fmt --all                           # fix formatting
cargo test -p drone-common                # tests for one crate
git config core.hooksPath .githooks       # once per clone: run scripts/check.sh before every commit
```

Rust lives in `~/.cargo/bin`, which may not be on `PATH` in a fresh shell (`scripts/check.sh` adds it).

## Code style

- **Always an explicit `return`**, in every function and closure, including the last expression
  (`return Ok(());`, `|x| return x + 1`). Enforced by `clippy::implicit_return`.
- Match the surrounding code: short doc comments that say *why*, plain-language names.
- Sequence numbers and session-clock times use `wrapping_add` / `wrapping_sub`, never plain `+`/`-`.

## Crate boundaries (protocol.md §7)

- `drone-common` is `no_std` with no heap: types, codes, encode/decode, COBS, CRC. No behaviour,
  no I/O, no clocks. Decoded messages own their data (fixed arrays), no borrowed lifetimes.
- Behaviour both sides need goes in `drone-link` (created when first needed), never in `drone-common`.
- Deciding whether to obey a request, timers, I/O: stays in `fc` / `client`.
- Incoming data is validated during decoding, always, also in release builds. `debug_assert!` is
  only for our own assumptions. Debug builds may add logs/asserts but never decide differently.

## Working agreements

- Test first: write the failing test, watch it fail, then implement.
- Every message in protocol.md has its own packet diagram, and diagrams must match their
  "where:" field lists. Re-check them after any protocol change.
- The user is new to drones: explain terms and spell out acronyms in docs and replies.
- Commits use the personal identity (`bram.simons@hotmail.com`, set in the repo's git config).
- Work on a branch and open a pull request for review; don't commit to `main` directly.
