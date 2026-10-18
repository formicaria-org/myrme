#!/usr/bin/env bash
# Run one method's live eval against one model: start a llama-server for it, run the eval test with
# recording on, stop the server. The answers are written back into methods/<method>/eval/cases.json.
#
#   pixi run eval -- <method> <gguf> <name> [gpu-layers]
#   pixi run eval -- meetings agents/models/LFM2.5-VL-450M-Q8_0.gguf lfm2.5-vl-450m 99
#
# The server binary is $LLAMA_SERVER, else agents/runtime/llama-server (pixi run fetch-model puts it
# there). Port $EVAL_PORT, default 8091. Context 8192, the laptop's window. Nothing is downloaded.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
METHOD="${1:?method}"; GGUF="${2:?gguf path}"; NAME="${3:?model name}"; NGL="${4:-99}"
BIN="${LLAMA_SERVER:-$HERE/runtime/llama-server}"
PORT="${EVAL_PORT:-8091}"
[ -x "$BIN" ] || { echo "no llama-server at $BIN (set LLAMA_SERVER)" >&2; exit 1; }
LOG="$(mktemp)"
LD_LIBRARY_PATH="$(dirname "$BIN")${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
  "$BIN" -m "$GGUF" --host 127.0.0.1 --port "$PORT" -c 8192 -ngl "$NGL" --no-warmup >"$LOG" 2>&1 &
PID=$!
trap 'kill $PID 2>/dev/null || true; rm -f "$LOG"' EXIT
for _ in $(seq 1 180); do
  grep -q "listening on" "$LOG" && break
  kill -0 $PID 2>/dev/null || { cat "$LOG" >&2; exit 1; }
  sleep 1
done
FM_MODEL_PORT="$PORT" FM_MODEL="$NAME" FM_EVAL_RECORD=1 \
  cargo test -q -p fm-agent --test "eval_$METHOD" -- --ignored --nocapture
