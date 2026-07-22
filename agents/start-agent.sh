#!/usr/bin/env bash
#
# Start the local study agent alongside formicaria — the web-search proxy + the model/@name watcher.
# fm-serve spawns THIS when the agent is enabled; a user never runs it. It dies with formicaria:
# the watcher (agent-serve) stops the model when fm-serve stops answering, and this then cleans up
# the proxy. Deleting agents/ makes formicaria byte-identical (nothing here to spawn).
#
#   bash agents/start-agent.sh [fm-serve-port]     # default 8765
#
set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SERVE_PORT="${1:-8765}"
PROXY_PORT="${2:-8888}"

# One persistent log for the whole agent stack — so "why didn't it answer?" is answerable. It holds
# the watcher's turn/timing lines, the search proxy, AND the model server's own stdout/stderr (which
# the watcher inherits), so a model crash is visible here too. Under runtime/ (gitignored); rolled if
# it grows past ~5 MB so it can never fill the disk.
mkdir -p "$HERE/runtime"
LOG="$HERE/runtime/agent.log"
if [ -f "$LOG" ] && [ "$(wc -c <"$LOG" 2>/dev/null || echo 0)" -gt 5000000 ]; then
  mv -f "$LOG" "$LOG.1" 2>/dev/null || true
fi
echo "=== agent stack starting $(date -u +%Y-%m-%dT%H:%M:%SZ) (fm-serve :$SERVE_PORT, proxy :$PROXY_PORT) ===" >>"$LOG"

# Web search proxy in the background; ensure it dies when we do. Its output joins the shared log.
python3 "$HERE/search-proxy.py" "$PROXY_PORT" >>"$LOG" 2>&1 &
PROXY=$!
trap 'kill "$PROXY" 2>/dev/null || true' EXIT INT TERM

# The model + @name watcher in the foreground. It self-terminates (stopping the model) when
# fm-serve stops answering, which returns here and fires the trap above. Its output (and the model
# server's, which it inherits) is teed to the log and still shown on the console for the dev loop.
bash "$HERE/agent-serve.sh" --serve-port "$SERVE_PORT" --searxng-port "$PROXY_PORT" 2>&1 | tee -a "$LOG"
