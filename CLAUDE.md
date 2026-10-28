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
  (`return Ok(());`, `|x| return x + 1`). Enforced by `clippy::implicit_return`. `if`/`match` used as a value
  (`let x = if a { 1 } else { 2 };`) stays idiomatic Rust.
- Few comments: only ones that add something the code doesn't already say (why, units, spec references,
  non-obvious edge cases). No comments restating the name or signature.
- Enum variant comments go on the same line as the variant (`ZeroByte, // encoded data contained 0x00`).
- Line length is 120 (`rustfmt.toml`).
- Sequence numbers, Request IDs and session-clock times use `wrapping_add` / `wrapping_sub`, never plain `+`/`-`.
- Define everything in one place: a type, its fields, its wire code and the names it allows are written once
  (e.g. a message's struct, type byte and refusal reasons in the `messages!` list). Reuse or re-export instead of
  defining a second copy (`domain::Sticks` re-exports the Sticks message), and never keep two lists that must
  stay in step by hand.
- Code reads at a glance. Watch for awkward shapes, when writing and when reviewing: nested wrappers
  (`Result<Result<(), Failure>, E>`, values like `Ok(Err(..))`), match arms that are only right because of the arms
  above them, two types that together mean one thing. Fix the type (e.g. one flat `Outcome` enum) so each match line
  reads as one rule. Reviews report these alongside bugs.

## Crate boundaries (protocol.md §7)

- `drone-common` is `no_std` with no heap: types, codes, encode/decode, COBS, CRC. No behaviour,
  no I/O, no clocks. Decoded messages own their data (fixed arrays), no borrowed lifetimes.
- Behaviour both sides need goes in `drone-link`, never in `drone-common`.
- Deciding whether to obey a request, timers, I/O: stays in `fc` / `client`.
- Decoded messages are typed (enums, named `bool`s for bit fields), not raw bytes; see protocol.md §7.1.
- Incoming data is validated during decoding, always, also in release builds. `debug_assert!` is
  only for our own assumptions. Debug builds may add logs/asserts but never decide differently.

## Working agreements

- Test first: write the failing test, watch it fail, then implement.
- Tests live in each crate's `tests/` folder (e.g. `common/tests/cobs.rs`), not in `#[cfg(test)]` modules in the
  source files. They test through the public API only; private functions are covered implicitly, never tested directly.
- Every message in protocol.md has its own packet diagram, and diagrams must match their
  "where:" field lists. Re-check them after any protocol change.
- The user is new to drones: explain terms and spell out acronyms in docs and replies.
- Commits use the personal identity (`bram.simons@hotmail.com`, set in the repo's git config).
- Work on a branch and open a pull request for review; don't commit to `main` directly. Merge with squash (one commit
  per pull request on `main`).
