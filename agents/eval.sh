#!/usr/bin/env bash
# Run one method's live eval against one model: start a llama-server for it, run the eval test with
# recording on, stop the server. The answers are written back into methods/<method>/eval/cases.json.
#
#   pixi run eval -- <method> <gguf> <name> [gpu-layers]
#   pixi run eval -- meetings agents/models/LFM2.5-VL-450M-Q8_0.gguf lfm2.5-vl-450m 99
#
# The server binary is $LLAMA_SERVER, else agents/runtime/llama-server (pixi run fetch-model puts it
# there). Port $EVAL_PORT, default 8091. Context 8192, the laptop's window. Nothing is downloaded.
# $EVAL_MMPROJ names a vision projector to load beside the weights (the image eval needs one);
# $EVAL_EXTRA adds server flags (e.g. --no-mmproj-offload, as the laptop catalogue entry asks).
# $EVAL_NO_RECORD leaves cases.json alone; $EVAL_SERVER_LOG keeps the server's log (its per-request
# timings) at that path — both for `agents/eval-speed.sh`. $EVAL_CTX sets the window (default 8192, the
# laptop's). $EVAL_ADB_DIR runs the server on a phone over USB instead (see below).
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
METHOD="${1:?method}"; GGUF="${2:?gguf path}"; NAME="${3:?model name}"; NGL="${4:-99}"
PORT="${EVAL_PORT:-8091}"
CTX="${EVAL_CTX:-8192}"
LOG="$(mktemp)"
if [ -n "${EVAL_ADB_DIR:-}" ]; then
  # **On a phone over USB** (2026-10-19): the server runs on the device from $EVAL_ADB_DIR, which
  # holds Myrme's Android runtime and the files named by GGUF/$EVAL_MMPROJ (their basenames), and
  # the port is forwarded so the evals here reach it. Its log streams back for the timings.
  # Only a process named exactly `llama-server` is stopped: the app's own is libllama-server.so.
  ADB="${ADB:-adb}"
  "$ADB" forward "tcp:$PORT" "tcp:$PORT" >/dev/null
  "$ADB" shell "cd $EVAL_ADB_DIR && LD_LIBRARY_PATH=$EVAL_ADB_DIR ./llama-server -m $(basename "$GGUF") \
    ${EVAL_MMPROJ:+--mmproj $(basename "$EVAL_MMPROJ")} --host 127.0.0.1 --port $PORT -c $CTX \
    -ngl $NGL --no-warmup ${EVAL_EXTRA:-}" >"$LOG" 2>&1 &
  PID=$!
  trap '"$ADB" shell pkill -x llama-server >/dev/null 2>&1 || true; "$ADB" forward --remove "tcp:$PORT" >/dev/null 2>&1 || true
        kill $PID 2>/dev/null || true; [ -n "${EVAL_SERVER_LOG:-}" ] && cp "$LOG" "$EVAL_SERVER_LOG"; rm -f "$LOG"' EXIT
else
  BIN="${LLAMA_SERVER:-$HERE/runtime/llama-server}"
  [ -x "$BIN" ] || { echo "no llama-server at $BIN (set LLAMA_SERVER)" >&2; exit 1; }
  LD_LIBRARY_PATH="$(dirname "$BIN")${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}" \
    "$BIN" -m "$GGUF" ${EVAL_MMPROJ:+--mmproj "$EVAL_MMPROJ"} --host 127.0.0.1 --port "$PORT" -c "$CTX" \
    -ngl "$NGL" --no-warmup ${EVAL_EXTRA:-} >"$LOG" 2>&1 &
  PID=$!
  trap 'kill $PID 2>/dev/null || true; [ -n "${EVAL_SERVER_LOG:-}" ] && cp "$LOG" "$EVAL_SERVER_LOG"; rm -f "$LOG"' EXIT
fi
for _ in $(seq 1 180); do
  grep -q "listening on" "$LOG" && break
  kill -0 $PID 2>/dev/null || { cat "$LOG" >&2; exit 1; }
  sleep 1
done
REC="FM_EVAL_RECORD=1"; [ -n "${EVAL_NO_RECORD:-}" ] && REC="FM_EVAL_RECORD_OFF=1"
env FM_MODEL_PORT="$PORT" FM_MODEL="$NAME" "$REC" \
  cargo test -q -p fm-agent --test "eval_$METHOD" -- --ignored --nocapture
