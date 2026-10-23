#!/usr/bin/env bash
# The checks every change must pass: formatting, lints, tests.
# Run by the pre-commit hook (.githooks/pre-commit) and by CI (.github/workflows/ci.yml).
set -euo pipefail
cd "$(dirname "$0")/.."

# Git hooks and IDEs may run without the login shell's PATH; rustup installs into ~/.cargo/bin.
if ! command -v cargo >/dev/null && [ -x "$HOME/.cargo/bin/cargo" ]; then
    PATH="$HOME/.cargo/bin:$PATH"
fi

cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
