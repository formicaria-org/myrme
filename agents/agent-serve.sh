#!/usr/bin/env bash
#
# Serve the model tied to formicaria's life: launches it under the watchdog and stops it when
# fm-serve (default port 8765) stops answering — model on with the app, off with it.
#
#   pixi run agent-serve                          # default model, watch fm-serve on :8765
#   pixi run agent-serve -- --serve-port 8770     # a different fm-serve port
#   pixi run agent-serve -- --model lfm2.5-350m   # a different catalogued model
#
# The binary reads `models.toml` itself (the single manifest reader) — no awk here — resolving the
# model file, ctx, threads, and port; pass any flag through to override. First bare arg = a model name.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# First non-flag arg is a model name; the rest pass through to the binary as flags.
MODEL_ARGS=()
if [ "${1:-}" ] && [ "${1#-}" = "${1}" ]; then MODEL_ARGS=(--model "$1"); shift; fi

# Prefer the prebuilt release binary (works when launched from the icon, with no pixi/cargo on PATH);
# fall back to `cargo run` for the dev loop.
REPO="$(cd "$HERE/.." && pwd)"
BIN="$REPO/target/release/agent-serve"
if [ -x "$BIN" ]; then RUN=("$BIN"); else RUN=(cargo run -q -p fm-agent-run --bin agent-serve --); fi

exec "${RUN[@]}" --agents-dir "$HERE" "${MODEL_ARGS[@]}" "$@"
