#!/usr/bin/env bash
# How fast each method runs on this machine: the method evals against one model, answers not
# recorded, the server's own per-request timings summed into one table (markdown, on stdout).
# Speed changes are measured with this the way accuracy changes are measured with `pixi run eval`.
#
#   pixi run eval-speed -- <gguf> <name> [gpu-layers] [methods...]
#   pixi run eval-speed -- agents/models/Qwen3VL-4B-Instruct-Q4_K_M.gguf qwen3-vl-4b 0
#
# Methods default to meetings research edits recipe image; `image` needs $EVAL_MMPROJ. The runtime,
# port and extra flags are eval.sh's ($LLAMA_SERVER, $EVAL_PORT, $EVAL_EXTRA).
#
# Columns, per method: requests; mean prompt tokens processed (after any cache reuse); prefill
# tok/s; mean prefill ms (time to first token, near enough); mean generated tokens; decode tok/s;
# mean seconds per request; and the eval's wall-clock seconds, which includes Rust-side work.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GGUF="${1:?gguf path}"; NAME="${2:?model name}"; NGL="${3:-99}"; shift $(( $# < 3 ? $# : 3 ))
METHODS=("$@"); [ ${#METHODS[@]} -gt 0 ] || METHODS=(meetings research edits recipe image)
OUT="$(mktemp -d)"
echo "| method | requests | prompt tok | prefill tok/s | prefill ms | gen tok | decode tok/s | s/request | wall s |"
echo "|---|---|---|---|---|---|---|---|---|"
for m in "${METHODS[@]}"; do
  if [ "$m" = image ] && [ -z "${EVAL_MMPROJ:-}" ]; then echo "| image | — (no \$EVAL_MMPROJ) |||||||| "; continue; fi
  start=$(date +%s)
  EVAL_NO_RECORD=1 EVAL_SERVER_LOG="$OUT/$m.log" bash "$HERE/eval.sh" "$m" "$GGUF" "$NAME" "$NGL" \
    >"$OUT/$m.out" 2>&1 || echo "eval-speed: $m failed, see $OUT/$m.out" >&2
  wall=$(( $(date +%s) - start ))
  python3 -I - "$OUT/$m.log" "$m" "$wall" <<'PY'
import re, sys
log, method, wall = sys.argv[1], sys.argv[2], sys.argv[3]
pp, tg = [], []
for line in open(log, errors="replace"):
    m = re.search(r"(prompt eval|\beval) time =\s*([\d.]+) ms /\s*(\d+) tokens", line)
    if m:
        (pp if m.group(1) == "prompt eval" else tg).append((float(m.group(2)), int(m.group(3))))
n = len(pp)
if not n:
    print(f"| {method} | 0 |||||||{wall} |"); sys.exit()
ppms = sum(a for a, _ in pp); ppt = sum(b for _, b in pp)
tgms = sum(a for a, _ in tg); tgt = sum(b for _, b in tg)
rate = lambda t, ms: f"{t / ms * 1000:.0f}" if ms else "—"
print(f"| {method} | {n} | {ppt / n:.0f} | {rate(ppt, ppms)} | {ppms / n:.0f} | {tgt / max(len(tg), 1):.0f} "
      f"| {rate(tgt, tgms)} | {(ppms + tgms) / n / 1000:.1f} | {wall} |")
PY
done
rm -rf "$OUT"
