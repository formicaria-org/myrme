#!/usr/bin/env python3
"""formicaria model benchmark — measure a model on OUR case, not a synthetic one.

Generic `llama-bench` runs a 128/64 synthetic prompt. That is not what the study agent does. This
drives a **running `llama-server`** through formicaria's real workload — our actual system prompts
(mirrored from `crates/fm-agent/src/{lib,grounding}.rs`) over a fixed, offline case battery
(`cases/*.json`): a discussion chat turn, a grounded-research synthesis over a canned numbered source
pack, and a house-format note write. Every number it reports is **measured** — decode tok/s straight
from llama.cpp's own `timings.predicted_per_second`, token counts from `usage`, and the model
process's live RSS/VmHWM (+ VRAM on the GPU, + MemAvailable to show the admission-gate slack).

The server is driven exactly as the app drives it (`POST /v1/chat/completions`, temperature 0 for
repeatability), so the throughput is the throughput the agent actually gets. Results are appended to
`results.md` — kept for future reference, so "is model X worth it?" is a re-run, not a re-derivation.

  # laptop, against the already-running agent-serve model:
  python3 agents/bench/run.py --url http://127.0.0.1:8081 --model qwen3-4b-2507 --device laptop \
      --pid $(pgrep -x llama-server) --gpu >> agents/bench/results.md

  # phone: adb-forward the on-device llama-server, then sample RSS via adb:
  adb forward tcp:18081 tcp:8081
  python3 agents/bench/run.py --url http://127.0.0.1:18081 --model qwen3-1.7b --device phone \
      --adb-name llama-server >> agents/bench/results.md
"""
import argparse
import glob
import json
import os
import re
import subprocess
import time
import urllib.request

HERE = os.path.dirname(os.path.abspath(__file__))
ADB = os.environ.get("ADB", ".android/platform-tools/adb")


def _read_status(args):
    """(VmRSS_kb, VmHWM_kb) for the model process — locally or via adb — or (None, None)."""
    if args.pid:
        try:
            text = open(f"/proc/{args.pid}/status").read()
        except OSError:
            return (None, None)
    elif args.adb_name:
        pid = subprocess.run([ADB, "shell", "pidof", args.adb_name],
                             capture_output=True, text=True).stdout.strip().split()
        if not pid:
            return (None, None)
        text = subprocess.run([ADB, "shell", "cat", f"/proc/{pid[0]}/status"],
                              capture_output=True, text=True).stdout
    else:
        return (None, None)

    def field(key):
        for ln in text.splitlines():
            if ln.startswith(key):
                return int(ln.split()[1])
        return None

    return (field("VmRSS:"), field("VmHWM:"))


def _mem_available_kb(args):
    if args.adb_name:
        text = subprocess.run([ADB, "shell", "cat", "/proc/meminfo"], capture_output=True, text=True).stdout
    else:
        text = open("/proc/meminfo").read()
    for ln in text.splitlines():
        if ln.startswith("MemAvailable:"):
            return int(ln.split()[1])
    return None


def _vram_used_mb():
    try:
        out = subprocess.run(["nvidia-smi", "--query-gpu=memory.used", "--format=csv,noheader,nounits"],
                             capture_output=True, text=True).stdout.strip().splitlines()
        return int(out[0])
    except Exception:
        return None


def _mb(kb):
    return None if kb is None else round(kb / 1024)


def run_case(url, case, n_predict, no_think):
    payload = {
        "messages": [
            {"role": "system", "content": case["system"]},
            {"role": "user", "content": case["user"]},
        ],
        "n_predict": n_predict,
        "temperature": 0,          # repeatable: same model+case → same output → same numbers
        "cache_prompt": False,     # measure a cold prompt eval, like a fresh turn
        "stream": False,
    }
    if no_think:
        # A study assistant wants a DIRECT answer, not a reasoning transcript. Thinking models
        # (Qwen3, …) otherwise spend the whole token budget inside <think> and return an empty
        # `content` — a config bug, not a model verdict. Ignored by non-thinking templates.
        payload["chat_template_kwargs"] = {"enable_thinking": False}
    if case.get("tools"):
        # Tool/search-call cases: hand the model the tool schema and see if it emits a well-formed
        # call (or correctly declines) — the BFCL-style "decide whether to search" axis.
        payload["tools"] = case["tools"]
    req = urllib.request.Request(url + "/v1/chat/completions", data=json.dumps(payload).encode(),
                                 headers={"Content-Type": "application/json"})
    t0 = time.time()
    d = json.load(urllib.request.urlopen(req, timeout=300))
    wall = time.time() - t0
    tim = d.get("timings", {})
    msg = d["choices"][0]["message"]
    content = msg.get("content") or ""
    reasoning = msg.get("reasoning_content") or ""
    finish = d["choices"][0].get("finish_reason")
    tool_calls = msg.get("tool_calls") or []
    return {
        "wall_s": round(wall, 2),
        "prompt_n": tim.get("prompt_n"),
        "gen_n": tim.get("predicted_n"),
        "decode_tps": round(tim.get("predicted_per_second") or 0, 1),
        "prompt_tps": round(tim.get("prompt_per_second") or 0, 1),
        "finish": finish,
        "quality": _quality(case, content, reasoning, finish, tool_calls),
        "content": content,
    }


def _norm(s):
    """Whitespace-collapsed, lowercased — so a verbatim match survives reflowing/casing."""
    return " ".join((s or "").split()).lower()


def _grounding(content, sources):
    """VERIFY, don't count: is each '> quote' a verbatim substring of the source its claim cited?
    `sources` = {"1": text, "2": text, …}. Returns (claims, quotes, verbatim, miscited, hallucinated).
    verbatim = quote found in the cited source (real grounding); miscited = verbatim but from a
    *different* source than the [n] it cited; hallucinated = quote in NO source (fabricated)."""
    norm_src = {k: _norm(v) for k, v in (sources or {}).items()}
    claims = quotes = verbatim = miscited = hallucinated = 0
    last_cite = None
    for ln in content.splitlines():
        s = ln.strip()
        if s.startswith("- "):
            claims += 1
            m = re.findall(r"\[(\d+)\]", s)
            last_cite = m[-1] if m else None
        elif s.startswith("> "):
            quotes += 1
            q = _norm(s[2:])
            if len(q) < 5:
                continue
            if last_cite and last_cite in norm_src and q in norm_src[last_cite]:
                verbatim += 1
            elif any(q in t for t in norm_src.values()):
                miscited += 1
            else:
                hallucinated += 1
    return claims, quotes, verbatim, miscited, hallucinated


def _quality(case, content, reasoning="", finish=None, tool_calls=None):
    """A small MEASURED signal per case — correctness where we can check it deterministically."""
    tool_calls = tool_calls or []
    check = case.get("check", case.get("name"))
    c = content.strip()

    # tool-call cases score on the CALL, so an empty content with a call is fine.
    if check == "toolcall":
        expect = case.get("expect_tool")  # a function name, or null = "must NOT call"
        names = [tc.get("function", {}).get("name") for tc in tool_calls]
        if expect is None:
            return "ok (correctly did not call)" if not tool_calls else f"WRONG: called {names}"
        if not tool_calls:
            return "NO CALL (should have searched)"
        if expect in names:
            # a well-formed call also needs a non-empty argument object
            args = next((tc["function"].get("arguments") for tc in tool_calls
                         if tc.get("function", {}).get("name") == expect), "")
            return "CALL ok" if args and args.strip() not in ("", "{}") else "CALL (empty args)"
        return f"WRONG CALL: {names}"

    if not c:
        if reasoning.strip():
            return f"THINK-ONLY ({len(reasoning.split())}w reasoning, no answer)"
        return "EMPTY(length)" if finish == "length" else "EMPTY"

    if check == "research":
        cl, q, vb, mis, hal = _grounding(c, case.get("sources"))
        out = f"{cl} claims / **{vb}/{q} verbatim**"
        if mis:
            out += f" / {mis} MIS-CITED"
        if hal:
            out += f" / {hal} HALLUCINATED-quote"
        return out

    if check == "abstain":
        # Junk/irrelevant sources → the contract says answer with a single '-' line and NO citation.
        cited = len(re.findall(r"\[\d+\]", c))
        _, _, vb, _, hal = _grounding(c, case.get("sources"))
        if cited == 0 or (vb == 0 and hal == 0 and cited <= 1):
            return "ok (abstained — no fabricated citations)"
        return f"FABRICATED ({cited} cites, {hal} hallucinated-quotes on junk sources)"

    # chat + write: the KaTeX math rule ($…$, never \( \) / \[ \]) + no whole-answer fence.
    flags = []
    if ("\\(" in c) or ("\\[" in c):
        flags.append("BAD-MATH(\\(\\[)")
    if c.startswith("```") and c.rstrip().endswith("```"):
        flags.append("WHOLE-FENCED")
    return "ok" if not flags else " ".join(flags)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", required=True, help="running llama-server base URL")
    ap.add_argument("--model", required=True, help="model label for the results block")
    ap.add_argument("--device", required=True, choices=["laptop", "phone"])
    ap.add_argument("--pid", type=int, help="model process pid (local RSS sampling)")
    ap.add_argument("--adb-name", help="model process name for adb RSS sampling (e.g. llama-server)")
    ap.add_argument("--gpu", action="store_true", help="also sample nvidia-smi VRAM")
    ap.add_argument("--think", action="store_true",
                    help="leave thinking ON (default OFF: a study assistant wants a direct answer)")
    ap.add_argument("--n-predict", type=int, default=256)
    ap.add_argument("--date", default="", help="YYYY-MM-DD (Date is unavailable in some sandboxes)")
    args = ap.parse_args()

    cases = [json.load(open(p)) for p in sorted(glob.glob(os.path.join(HERE, "cases", "*.json")))]
    if not cases:
        raise SystemExit("no cases found in agents/bench/cases/")

    mem_before = _mem_available_kb(args)
    vram_before = _vram_used_mb() if args.gpu else None

    rows = []
    peak_rss = peak_hwm = 0
    peak_vram = 0
    for case in cases:
        r = run_case(args.url, case, args.n_predict, no_think=not args.think)
        rss, hwm = _read_status(args)
        if rss:
            peak_rss = max(peak_rss, rss)
        if hwm:
            peak_hwm = max(peak_hwm, hwm)
        if args.gpu:
            v = _vram_used_mb()
            if v:
                peak_vram = max(peak_vram, v)
        rows.append((case["name"], r))

    mem_min = _mem_available_kb(args)
    date = args.date or "undated"

    think = "thinking ON" if args.think else "thinking OFF (direct-answer)"
    print(f"\n### {args.device} · {args.model} · {date}  ({think})")
    gate = []
    if peak_rss:
        gate.append(f"model RSS {_mb(peak_rss)} MB / VmHWM {_mb(peak_hwm)} MB")
    if args.gpu and peak_vram:
        gate.append(f"VRAM {peak_vram} MB")
    if mem_min is not None:
        gate.append(f"MemAvailable during run: {round(mem_min/1024)} MB (before {round((mem_before or 0)/1024)} MB)")
    print("- " + " · ".join(gate))
    print()
    print("| case | prompt_n | gen_n | decode tok/s | prompt tok/s | wall s | quality |")
    print("|---|---|---|---|---|---|---|")
    for name, r in rows:
        print(f"| {name} | {r['prompt_n']} | {r['gen_n']} | **{r['decode_tps']}** | {r['prompt_tps']} | {r['wall_s']} | {r['quality']} |")
    decode = [r["decode_tps"] for _, r in rows if r["decode_tps"]]
    if decode:
        print(f"\n_mean decode: **{round(sum(decode)/len(decode), 1)} tok/s** over {len(decode)} cases._")


if __name__ == "__main__":
    main()
