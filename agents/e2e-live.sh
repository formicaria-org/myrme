#!/usr/bin/env bash
# **The whole thing, live**: a formicaria build that links Myrme, its own agent-serve, a real model on
# a real runtime, and a person's message — nothing stood in for. Where `pixi run integration` plays
# the model to test the door, this plays nothing.
#
#   FM_CHECKOUT=../formicaria MODELS=../formicaria/agents/models RUNTIME=<dir with llama-server> \
#     bash agents/e2e-live.sh [model name, default qwen3-4b-2507]
#
# FM_CHECKOUT is a formicaria checkout whose `fm-serve` and `agent-serve` are built in
# target/release (the cutover branch builds both from Myrme). MODELS holds the GGUF files; RUNTIME
# holds an extracted llama.cpp archive (Myrme's runtime release, or upstream's). Everything runs in a
# temporary home: no configuration, vault or model folder of the machine's is touched.
set -euo pipefail
: "${FM_CHECKOUT:?a formicaria checkout with target/release/{fm-serve,agent-serve}}"
: "${MODELS:?the folder holding the model files}"
: "${RUNTIME:?an extracted llama.cpp runtime}"
MODEL="${1:-qwen3-4b-2507}"
FM="$FM_CHECKOUT/target/release/fm-serve"; AGENT="$FM_CHECKOUT/target/release/agent-serve"
PORT="${E2E_PORT:-18765}"; MPORT="${E2E_MODEL_PORT:-18081}"
T="$(mktemp -d)"; mkdir -p "$T/home" "$T/vault" "$T/agents"
ln -s "$MODELS" "$T/agents/models"; ln -s "$RUNTIME" "$T/agents/runtime"
"$AGENT" --print-catalogue > "$T/agents/models.toml"
api() { curl -sf -X POST -H 'Content-Type: application/json' --data "$2" "http://127.0.0.1:$PORT/api/$1"; }

env -i PATH="$PATH" HOME="$T/home" XDG_CONFIG_HOME="$T/home/.config" FM_ADDR="127.0.0.1:$PORT" \
  FM_VAULT="$T/vault" FM_VAULTS="$T/vaults.json" FM_OPEN=0 FM_AUTO_SHUTDOWN=0 \
  GIT_AUTHOR_NAME="Owner" GIT_AUTHOR_EMAIL="owner@example.org" \
  GIT_COMMITTER_NAME="Owner" GIT_COMMITTER_EMAIL="owner@example.org" \
  "$FM" >"$T/fm.log" 2>&1 &
FMPID=$!
cleanup() { kill "${AGPID:-}" "$FMPID" 2>/dev/null || true; sleep 1; rm -rf "$T"; }
trap cleanup EXIT
for _ in $(seq 1 60); do api alive '{}' >/dev/null 2>&1 && break; sleep 0.5; done

BODY="Survey notes$(printf '\n\nA sentence of field notes about the wetland survey, round %s.' $(seq 1 40))

The last paragraph, which the model is never shown."
NOTE=$(api capture "$(python3 -c 'import json,sys; print(json.dumps({"body": sys.argv[1], "vault": ""}))' "$BODY")" \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["id"])')
echo "note $NOTE ($(printf '%s' "$BODY" | wc -c) characters)"

HOME="$T/home" "$AGENT" --agents-dir "$T/agents" --model "$MODEL" --serve-port "$PORT" \
  --model-port "$MPORT" --device laptop >"$T/agent.log" 2>&1 &
AGPID=$!
# A message already in a discussion when Myrme starts is history, not a question to it — so ask
# once it is listening, as a person would.
for _ in $(seq 1 180); do grep -q "listening — model warm" "$T/agent.log" && break
  kill -0 $AGPID 2>/dev/null || { echo "agent-serve exited:"; tail -20 "$T/agent.log"; exit 1; }; sleep 1; done
api reply "$(python3 -c 'import json,sys; print(json.dumps({"id": sys.argv[1], "body": "@Myrme-laptop add a final line: Next survey: 14 November /propose"}))' "$NOTE")" >/dev/null
echo "asked @Myrme-laptop to propose an edit; waiting for the answer..."

PROPOSAL=""
for _ in $(seq 1 240); do
  PROPOSAL=$(api proposal_for "{\"id\":\"$NOTE\"}" 2>/dev/null | python3 -c 'import json,sys; v=json.load(sys.stdin); print(v if isinstance(v,str) else (v or {}).get("id","") if isinstance(v,dict) else "")' 2>/dev/null || true)
  [ -n "$PROPOSAL" ] && [ "$PROPOSAL" != "None" ] && break
  kill -0 $AGPID 2>/dev/null || { echo "agent-serve exited:"; tail -20 "$T/agent.log"; exit 1; }
  sleep 1
done
[ -n "$PROPOSAL" ] && [ "$PROPOSAL" != "None" ] || { echo "no proposal after 4 minutes"; tail -30 "$T/agent.log"; exit 1; }
api get "{\"id\":\"$NOTE\"}" > "$T/note.json"
api proposal_content "{\"id\":\"$PROPOSAL\"}" > "$T/proposal.json"
if [ -n "${E2E_DEBUG:-}" ]; then cp "$T/note.json" "$T/proposal.json" "$T/agent.log" "$E2E_DEBUG/"; fi
cat "$T/proposal.json" | python3 -c '
import json, sys
body = json.load(sys.stdin)["body"]
original = sys.argv[1]
ok_kept = body.startswith(original.rstrip())
ok_line = body.rstrip().endswith("Next survey: 14 November")
print("proposal:", sys.argv[2])
print("  keeps the whole note:", ok_kept)
print("  ends with the new line:", ok_line)
print("  ...", body[-120:].replace("\n", " / "))
sys.exit(0 if ok_kept and ok_line else 1)
' "$BODY" "$PROPOSAL"
echo "thread:"; api thread "{\"id\":\"$NOTE\"}" | python3 -c '
import json,sys
for m in json.load(sys.stdin).get("messages", []): print("  -", (m.get("author") or m.get("authorName") or "?"), ":", (m.get("body") or "")[:110])'
grep -m1 -i "llguidance\|llama-server" "$T/agent.log" >/dev/null && true
echo "PASS: live end to end"
