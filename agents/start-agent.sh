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

# Web search proxy in the background; ensure it dies when we do.
python3 "$HERE/search-proxy.py" "$PROXY_PORT" >/dev/null 2>&1 &
PROXY=$!
trap 'kill "$PROXY" 2>/dev/null || true' EXIT INT TERM

# The model + @name watcher in the foreground. It self-terminates (stopping the model) when
# fm-serve stops answering, which returns here and fires the trap above.
bash "$HERE/agent-serve.sh" --serve-port "$SERVE_PORT" --searxng-port "$PROXY_PORT"
