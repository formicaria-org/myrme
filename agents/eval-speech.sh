#!/usr/bin/env bash
# Run M4's live eval: build the audio if missing, start a whisper-server on the given weights, run
# the eval with recording on, stop the server.
#
#   pixi run eval-speech -- <weights .bin> <name> [extra whisper-server flags]
#
# The server binary is $WHISPER_SERVER, else agents/runtime/whisper-server. Port $EVAL_PORT (8092).
# `-l auto`, as formicaria runs it: the language is the reader's to find.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_WEIGHTS="${1:?weights .bin}"; NAME="${2:?name}"; shift 2
BIN="${WHISPER_SERVER:-$HERE/runtime/whisper-server}"
PORT="${EVAL_PORT:-8092}"
python -I "$HERE/eval-data.py" check-speech
LOG="$(mktemp)"
LD_LIBRARY_PATH="$(dirname "$BIN")${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
  "$BIN" -m "$BIN_WEIGHTS" --host 127.0.0.1 --port "$PORT" -l auto "$@" >"$LOG" 2>&1 &
PID=$!
trap 'kill $PID 2>/dev/null || true; rm -f "$LOG"' EXIT
for _ in $(seq 1 120); do
  grep -qi "listening" "$LOG" && break
  kill -0 $PID 2>/dev/null || { cat "$LOG" >&2; exit 1; }
  sleep 1
done
FM_WHISPER_PORT="$PORT" FM_WHISPER_MODEL="$NAME" FM_EVAL_RECORD=1 \
  cargo test -q -p fm-agent --test eval_audio -- --ignored --nocapture
