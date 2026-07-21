#!/usr/bin/env bash
#
# Fetch the agent's local model runtime + a model into the gitignored folders beside this script.
# Everything lands INSIDE the repo (agents/runtime, agents/models) — nothing is written outside it,
# and nothing large is committed. Idempotent: an already-present runtime or model is left alone.
#
#   bash agents/fetch.sh            # fetch the `default` model from models.toml
#   bash agents/fetch.sh lfm2.5-1.2b   # fetch a named model
#   pixi run fetch-model [name]     # the same, via pixi
#
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONF="$HERE/models.toml"
RUNTIME_DIR="$HERE/runtime"
MODELS_DIR="$HERE/models"
RUNTIME_BIN="$RUNTIME_DIR/llama-server"

[ -f "$CONF" ] || { echo "missing $CONF" >&2; exit 1; }

# A top-level `key = "value"` (or bare value) from the manifest.
conf_get() {
  awk -F'=' -v k="$1" '
    /^\[/ { top=0 } NR==1 { top=1 }
    $0 ~ "^"k" *=" { v=$2; gsub(/^[ "]+|[ "]+$/,"",v); print v; exit }
  ' "$CONF"
}

# The (repo, file) for a model name, read positionally from its [[models]] block.
model_fields() {
  awk -v want="$1" '
    /^\[\[models\]\]/ { name=""; repo=""; file="" }
    /^name *=/ { v=$0; sub(/^name *= *"?/,"",v); sub(/"? *$/,"",v); name=v }
    /^repo *=/ { v=$0; sub(/^repo *= *"?/,"",v); sub(/"? *$/,"",v); repo=v }
    /^file *=/ { v=$0; sub(/^file *= *"?/,"",v); sub(/"? *$/,"",v); file=v;
                 if (name==want) { print repo "\t" file; exit } }
  ' "$CONF"
}

MODEL="${1:-$(conf_get default)}"
[ -n "$MODEL" ] || { echo "no model given and no default in models.toml" >&2; exit 1; }

IFS=$'\t' read -r REPO FILE < <(model_fields "$MODEL")
if [ -z "${REPO:-}" ] || [ -z "${FILE:-}" ]; then
  echo "unknown model '$MODEL' — not in models.toml" >&2
  echo "known models:" >&2
  awk '/^name *=/ { v=$0; sub(/^name *= *"?/,"",v); sub(/"? *$/,"",v); print "  " v }' "$CONF" >&2
  exit 1
fi

mkdir -p "$RUNTIME_DIR" "$MODELS_DIR"

# --- runtime (prebuilt llama-server + its shared libs) ---
if [ -x "$RUNTIME_BIN" ]; then
  echo "runtime already present: $RUNTIME_BIN"
else
  URL="$(conf_get runtime_url)"
  echo "fetching llama.cpp runtime…"
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  curl -fL --retry 2 -o "$tmp/rt.tar.gz" "$URL"
  tar xzf "$tmp/rt.tar.gz" -C "$tmp"
  sub="$(find "$tmp" -maxdepth 1 -type d -name 'llama-*' | head -1)"
  [ -n "$sub" ] || { echo "unexpected runtime archive layout" >&2; exit 1; }
  cp -a "$sub"/. "$RUNTIME_DIR"/
  chmod +x "$RUNTIME_BIN"
  echo "runtime → $RUNTIME_BIN"
fi

# --- model weights ---
DEST="$MODELS_DIR/$FILE"
if [ -f "$DEST" ]; then
  echo "model already present: $DEST"
else
  echo "fetching $MODEL  ($REPO/$FILE)…"
  curl -fL --retry 2 -o "$DEST.part" "https://huggingface.co/$REPO/resolve/main/$FILE"
  mv "$DEST.part" "$DEST"
  echo "model → $DEST"
fi

PORT="$(conf_get port)"; CTX="$(conf_get ctx)"; THREADS="$(conf_get threads)"
cat <<EOF

Ready. Serve it (bounded, localhost-only) with:

  LD_LIBRARY_PATH="$RUNTIME_DIR" "$RUNTIME_BIN" \\
    -m "$DEST" --host 127.0.0.1 --port ${PORT:-8081} -c ${CTX:-2048} -t ${THREADS:-4}

Then point the agent at it: OpenAiStep::local(${PORT:-8081}, "$MODEL").
EOF
