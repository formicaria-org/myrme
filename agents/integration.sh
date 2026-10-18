#!/usr/bin/env bash
# Myrme against a real formicaria: build `fm-serve` from the formicaria release Myrme pins, then run
# the integration test (crates/fm-agent-run/tests/integration_fm.rs) against it. No model is needed:
# the test answers for the model itself, so what is checked is the door between the two programs.
#
#   pixi run integration
#   FM_CHECKOUT=~/fm-myrme pixi run integration     # a local formicaria branch instead of the pin
#
# The pin is the one `fm-fetch` is taken at (crates/fm-agent-run/Cargo.toml), so there is one
# formicaria version in this repo. The checkout lives in .fm/ (gitignored), never in a developer's
# own formicaria clone. Network: one shallow clone per tag, then nothing.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TAG="$(sed -n 's/^fm-fetch = { git = "[^"]*", tag = "\([^"]*\)".*/\1/p' "$HERE/crates/fm-agent-run/Cargo.toml")"
[ -n "$TAG" ] || { echo "no formicaria tag found in crates/fm-agent-run/Cargo.toml" >&2; exit 1; }
SRC="$HERE/.fm/$TAG"
# FM_CHECKOUT=<path>: an existing formicaria checkout instead of the pinned tag — how a formicaria
# branch that links Myrme from this repository (the cutover) is tested from this side too.
if [ -n "${FM_CHECKOUT:-}" ]; then
  SRC="$(cd "$FM_CHECKOUT" && pwd)"
  TAG="$(git -C "$SRC" rev-parse --abbrev-ref HEAD)@$(git -C "$SRC" rev-parse --short HEAD)"
  echo "integration: formicaria from $SRC ($TAG)" >&2
elif [ ! -d "$SRC/.git" ]; then
  git clone -q --depth 1 --branch "$TAG" https://github.com/formicaria-org/formicaria.git "$SRC"
fi
# Default features: the build formicaria ships. No UI is embedded (none is needed over /api).
cargo build -q --release --manifest-path "$SRC/Cargo.toml" -p fm-serve
FM_SERVE_BIN="$SRC/target/release/fm-serve" FM_SRC="$SRC" FM_TAG="$TAG" \
  cargo test -q -p fm-agent-run --test integration_fm -- --ignored --nocapture
