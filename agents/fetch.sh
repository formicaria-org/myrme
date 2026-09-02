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

# The (repo, file, revision, sha256) for a model name, from its [[models]] block. Emitted at the end
# of the block rather than on the `file` line, so the fields may appear in any order — the old version
# printed as soon as it saw `file`, which made `revision`/`sha256` unreadable by construction.
model_fields() {
  awk -v want="$1" '
    function emit() { if (name==want && !done) { print repo "\t" file "\t" rev "\t" sha "\t" mmproj; done=1 } }
    /^\[\[models\]\]/ { emit(); name=""; repo=""; file=""; rev=""; sha=""; mmproj="" }
    /^name *=/     { v=$0; sub(/^name *= *"?/,"",v);     sub(/"? *$/,"",v); name=v }
    /^repo *=/     { v=$0; sub(/^repo *= *"?/,"",v);     sub(/"? *$/,"",v); repo=v }
    /^file *=/     { v=$0; sub(/^file *= *"?/,"",v);     sub(/"? *$/,"",v); file=v }
    /^revision *=/ { v=$0; sub(/^revision *= *"?/,"",v); sub(/"? *$/,"",v); rev=v }
    /^sha256 *=/   { v=$0; sub(/^sha256 *= *"?/,"",v);   sub(/"? *$/,"",v); sha=v }
    /^mmproj *=/   { v=$0; sub(/^mmproj *= *"?/,"",v);   sub(/"? *$/,"",v); mmproj=v }
    END { emit() }
  ' "$CONF"
}

# SHA-256 of a file, whichever tool this machine spells it with.
sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d" " -f1
  elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | cut -d" " -f1
  else echo ""; fi
}

MODEL="${1:-$(conf_get default)}"
[ -n "$MODEL" ] || { echo "no model given and no default in models.toml" >&2; exit 1; }

IFS=$'\t' read -r REPO FILE REV SHA MMPROJ < <(model_fields "$MODEL")
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
  # GPU by default, CPU fallback: take the Vulkan build when `gpu` is not "off" AND a Vulkan loader is
  # installed (a discrete GPU then offloads via `-ngl`; the same binary still runs on CPU where there is
  # no GPU). Otherwise the portable CPU build. So a GPU machine gets GPU speed with no configuration.
  GPU="$(conf_get gpu)"; GPU="${GPU:-auto}"
  URL="$(conf_get runtime_url_linux_x64)"
  if [ "$GPU" != "off" ] && ldconfig -p 2>/dev/null | grep -q 'libvulkan\.so'; then
    GPU_URL="$(conf_get runtime_url_linux_x64_gpu)"
    if [ -n "$GPU_URL" ]; then URL="$GPU_URL"; echo "GPU detected (Vulkan) — using the GPU runtime"; fi
  fi
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
  # **The pinned commit, not `main`.** `main` is a moving pointer: it can hand you different bytes
  # tomorrow than the ones this project measured, and the checksum below would then be right to
  # refuse them. The pair only works together — see the note above the [[models]] blocks.
  echo "fetching $MODEL  ($REPO/$FILE @ ${REV:-main})…"
  curl -fL --retry 2 -o "$DEST.part" "https://huggingface.co/$REPO/resolve/${REV:-main}/$FILE"
  if [ -n "${SHA:-}" ]; then
    GOT="$(sha256_of "$DEST.part")"
    if [ -z "$GOT" ]; then
      rm -f "$DEST.part"
      echo "no sha256sum/shasum on this machine, and $MODEL pins a checksum — refusing to install unverified weights" >&2
      exit 1
    fi
    if [ "$GOT" != "$SHA" ]; then
      rm -f "$DEST.part"
      echo "checksum mismatch for $MODEL: got $GOT, expected $SHA — deleted the partial download" >&2
      exit 1
    fi
    echo "checksum ok"
  fi
  mv "$DEST.part" "$DEST"
  echo "model → $DEST"
fi

# --- the multimodal projector, when this model has one --------------------------------------------
# Optional and additive: without it the same weights serve text-only, exactly as before, and the
# agent says `/describe` is unavailable rather than asking a blind model to read a picture. Fetched
# from the same pinned commit as the weights, for the same reason.
if [ -n "${MMPROJ:-}" ]; then
  PROJ="$MODELS_DIR/$MMPROJ"
  if [ -f "$PROJ" ]; then
    echo "projector already present: $PROJ"
  else
    echo "fetching projector ($REPO/$MMPROJ @ ${REV:-main})…"
    if curl -fL --retry 2 -o "$PROJ.part" "https://huggingface.co/$REPO/resolve/${REV:-main}/$MMPROJ"; then
      mv "$PROJ.part" "$PROJ"
      echo "projector → $PROJ   (images: /describe)"
    else
      # A missing projector is a missing *capability*, not a broken install: the model still serves
      # text. Say so and carry on rather than failing a fetch that otherwise succeeded.
      rm -f "$PROJ.part"
      echo "could not fetch the projector — the model will serve text-only and /describe will say so" >&2
    fi
  fi
fi

PORT="$(conf_get port)"; CTX="$(conf_get ctx)"; THREADS="$(conf_get threads)"
cat <<EOF

Ready. Serve it (bounded, localhost-only) with:

  LD_LIBRARY_PATH="$RUNTIME_DIR" "$RUNTIME_BIN" \\
    -m "$DEST"${MMPROJ:+ --mmproj "$MODELS_DIR/$MMPROJ"} \\
    --host 127.0.0.1 --port ${PORT:-8081} -c ${CTX:-2048} -t ${THREADS:-4}

Then point the agent at it: OpenAiStep::local(${PORT:-8081}, "$MODEL").
EOF
