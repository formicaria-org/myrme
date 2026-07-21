#!/usr/bin/env bash
#
# Serve the model tied to formicaria's life: launches it under the watchdog and stops it when
# fm-serve (default port 8765) stops answering — model on with the app, off with it.
#
#   pixi run agent-serve                 # default model, watch fm-serve on :8765
#   pixi run agent-serve -- --serve-port 8770   # a different fm-serve port
#
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONF="$HERE/models.toml"

conf_get() {
  awk -F'=' -v k="$1" '$0 ~ "^"k" *=" { v=$2; gsub(/^[ "]+|[ "]+$/,"",v); print v; exit }' "$CONF"
}
model_file() {
  awk -v want="$1" '
    /^\[\[models\]\]/ { n=""; f="" }
    /^name *=/ { n=$0; sub(/^name *= *"?/,"",n); sub(/"? *$/,"",n) }
    /^file *=/ { f=$0; sub(/^file *= *"?/,"",f); sub(/"? *$/,"",f); if (n==want) { print f; exit } }
  ' "$CONF"
}

# First non-flag arg is a model name; the rest pass through to the binary.
MODEL="$(conf_get default)"
if [ "${1:-}" ] && [ "${1#-}" = "${1}" ]; then MODEL="$1"; shift; fi
FILE="$(model_file "$MODEL")"
[ -n "$FILE" ] || { echo "unknown model '$MODEL' (not in models.toml)" >&2; exit 1; }
[ -f "$HERE/models/$FILE" ] || { echo "model not fetched — run 'pixi run fetch-model $MODEL'" >&2; exit 1; }

CTX="$(conf_get ctx)"; THREADS="$(conf_get threads)"; PORT="$(conf_get port)"

# Prefer the prebuilt release binary (works when launched from the icon, with no pixi/cargo on PATH);
# fall back to `cargo run` for the dev loop.
REPO="$(cd "$HERE/.." && pwd)"
BIN="$REPO/target/release/agent-serve"
if [ -x "$BIN" ]; then RUN=("$BIN"); else RUN=(cargo run -q -p fm-agent-run --bin agent-serve --); fi

exec "${RUN[@]}" \
  --model-gguf "$HERE/models/$FILE" --runtime "$HERE/runtime" \
  --model-port "${PORT:-8081}" --ctx "${CTX:-2048}" --threads "${THREADS:-4}" "$@"
