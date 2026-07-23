#!/usr/bin/env bash
#
# Stage the audio→transcript runtime (whisper.cpp) into the gitignored folders beside this script —
# the same layout `fetch.sh` uses for the model runtime. Manual-only, LAPTOP v1. Idempotent: an
# already-present model or binary is left alone.
#
#   bash agents/fetch-whisper.sh     # or: pixi run fetch-whisper
#
# It always fetches the (small, reliably hosted) ggml weights. The `whisper-server` binary is fetched
# only if `whisper_runtime_url` is set in models.toml (whisper.cpp ships no portable Linux server
# tarball the way llama.cpp does); otherwise it prints how to place it. `agent-serve --whisper-port`
# errors clearly if the binary is missing, so this never half-works silently.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONF="$HERE/models.toml"
RUNTIME_DIR="$HERE/runtime"
MODELS_DIR="$HERE/models"

[ -f "$CONF" ] || { echo "missing $CONF" >&2; exit 1; }

conf_get() {
  awk -F'=' -v k="$1" '
    /^\[/ { top=0 } NR==1 { top=1 }
    $0 ~ "^"k" *=" { v=$2; gsub(/^[ "]+|[ "]+$/,"",v); print v; exit }
  ' "$CONF"
}

MODEL="$(conf_get whisper_model)"
REPO="$(conf_get whisper_model_repo)"
FILE="$(conf_get whisper_model_file)"
RUNTIME_URL="$(conf_get whisper_runtime_url)"
[ -n "$MODEL" ] && [ -n "$REPO" ] && [ -n "$FILE" ] || {
  echo "models.toml is missing whisper_model / whisper_model_repo / whisper_model_file" >&2; exit 1;
}

mkdir -p "$RUNTIME_DIR" "$MODELS_DIR"

# --- weights (serve.rs expects models/<whisper_model>.bin) ---
DEST="$MODELS_DIR/$MODEL.bin"
if [ -f "$DEST" ]; then
  echo "whisper model already present: $DEST"
else
  echo "fetching whisper model ($REPO/$FILE)…"
  curl -fL --retry 2 -o "$DEST.part" "https://huggingface.co/$REPO/resolve/main/$FILE"
  mv "$DEST.part" "$DEST"
  echo "whisper model → $DEST"
fi

# --- runtime binary (whisper-server) ---
WBIN="$RUNTIME_DIR/whisper-server"
if [ -x "$WBIN" ]; then
  echo "whisper-server already present: $WBIN"
elif [ -n "$RUNTIME_URL" ]; then
  echo "fetching whisper-server runtime…"
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  curl -fL --retry 2 -o "$tmp/w.tar.gz" "$RUNTIME_URL"
  tar xzf "$tmp/w.tar.gz" -C "$tmp"
  found="$(find "$tmp" -type f -name 'whisper-server' | head -1)"
  [ -n "$found" ] || { echo "no 'whisper-server' in $RUNTIME_URL" >&2; exit 1; }
  # Copy the binary plus any shared libs beside it (LD_LIBRARY_PATH points at runtime/).
  cp -a "$(dirname "$found")"/. "$RUNTIME_DIR"/
  chmod +x "$WBIN"
  echo "whisper-server → $WBIN"
else
  cat >&2 <<EOF

whisper-server binary is NOT staged. Either:
  • set 'whisper_runtime_url' in agents/models.toml to a tarball that contains a 'whisper-server', or
  • build it from whisper.cpp and copy the binary (with its shared libs) to:
        $WBIN

Then serve with:  pixi run agent-serve -- --whisper-port 8082
EOF
fi

PORT_HINT=8082
cat <<EOF

Ready (weights staged). When the binary is in place, enable audio→transcript with:

  pixi run agent-serve -- --whisper-port ${PORT_HINT} --whisper-model ${MODEL}

Then in a note's discussion, tap Transcribe on an audio artifact (or type: @<agent> /transcribe <asset>).
EOF
