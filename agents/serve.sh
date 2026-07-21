#!/usr/bin/env bash
#
# Serve a fetched model as a bounded, localhost-only llama-server — the warm server the agent talks
# to. Runs in the foreground (Ctrl-C stops it). Bounds: small context (KV cache), capped threads.
#
#   bash agents/serve.sh            # the default model from models.toml
#   bash agents/serve.sh lfm2.5-350m
#   pixi run serve-model [name]
#
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONF="$HERE/models.toml"
RUNTIME="$HERE/runtime"

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

MODEL="${1:-$(conf_get default)}"
FILE="$(model_file "$MODEL")"
GGUF="$HERE/models/$FILE"
BIN="$RUNTIME/llama-server"

[ -n "$FILE" ] || { echo "unknown model '$MODEL' (not in models.toml)" >&2; exit 1; }
[ -x "$BIN" ] || { echo "runtime missing — run 'pixi run fetch-model' first" >&2; exit 1; }
[ -f "$GGUF" ] || { echo "model not fetched — run 'pixi run fetch-model $MODEL' first" >&2; exit 1; }

PORT="$(conf_get port)"; CTX="$(conf_get ctx)"; THREADS="$(conf_get threads)"
echo "serving $MODEL on 127.0.0.1:${PORT:-8081} (Ctrl-C to stop)…"
exec env LD_LIBRARY_PATH="$RUNTIME" "$BIN" \
  -m "$GGUF" --host 127.0.0.1 --port "${PORT:-8081}" -c "${CTX:-2048}" -t "${THREADS:-4}" --no-warmup
