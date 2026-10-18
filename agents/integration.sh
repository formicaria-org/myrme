#!/usr/bin/env bash
# Myrme against a real formicaria: build `fm-serve` from the formicaria release Myrme pins, then run
# the integration test (crates/fm-agent-run/tests/integration_fm.rs) against it. No model is needed:
# the test answers for the model itself, so what is checked is the door between the two programs.
#
#   pixi run integration
#
# The pin is the one `fm-fetch` is taken at (crates/fm-agent-run/Cargo.toml), so there is one
# formicaria version in this repo. The checkout lives in .fm/ (gitignored), never in a developer's
# own formicaria clone. Network: one shallow clone per tag, then nothing.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAG="$(sed -n 's/^fm-fetch = { git = "[^"]*", tag = "\([^"]*\)".*/\1/p' "$HERE/crates/fm-agent-run/Cargo.toml")"
[ -n "$TAG" ] || { echo "no formicaria tag found in crates/fm-agent-run/Cargo.toml" >&2; exit 1; }
SRC="$HERE/.fm/$TAG"
if [ ! -d "$SRC/.git" ]; then
  git clone -q --depth 1 --branch "$TAG" https://github.com/formicaria-org/formicaria.git "$SRC"
fi
# Default features: the build formicaria ships. No UI is embedded (none is needed over /api).
cargo build -q --release --manifest-path "$SRC/Cargo.toml" -p fm-serve
FM_SERVE_BIN="$SRC/target/release/fm-serve" FM_SRC="$SRC" FM_TAG="$TAG" \
  cargo test -q -p fm-agent-run --test integration_fm -- --ignored --nocapture
