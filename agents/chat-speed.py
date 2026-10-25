"""How fast chat answers on this machine: three turns on one note, sent the way `StudyAssistant::turn`
sends them (the provided notes, then the conversation, then the ask), streamed. Per turn it prints the
tokens the server read and reused, the first token's time, the whole reply's time and the writing
rate, against chat's bar (`methods/chat/METHOD.md`, *Wait*: first words within 2 s, then 8 tok/s).

Chat has no eval (a reply has no single right answer), so this is its speed table, the way
`agents/eval-speed.sh` is every other method's. The note is built from the public eval cases (research
sources, the long edit notes, recipe transcripts) to about 1,500 tokens; nothing personal is read.

Start the model as the app does (its catalogue flags included, e.g. `-ub 128` for qwen3.5-0.8b), then:

    python3 -I agents/chat-speed.py <port>
"""
import json
import pathlib
import re
import sys
import time
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
PORT = int(sys.argv[1]) if len(sys.argv) > 1 else 8081
FIRST_WORDS_S, RATE = 2.0, 8.0

# The chat prompt, read from the crate so this cannot drift from what production sends.
src = (ROOT / "crates/fm-agent/src/prompts.rs").read_text()
body = re.search(r'pub const CHAT_INSTRUCTION: &str = "\\\n(.*?)";', src, re.S).group(1)
CHAT_INSTRUCTION = re.sub(r"\\\n\s*", "", body).replace('\\"', '"').replace("\\\\", "\\")


def note():
    m = ROOT / "methods"
    parts = []
    for case in json.loads((m / "research/eval/cases.json").read_text())["cases"]:
        parts += [s["text"] for s in case.get("sources", []) if isinstance(s, dict)]
    edits = json.loads((m / "edits/eval/cases.json").read_text())["cases"]
    parts += [c["body"] for c in sorted(edits, key=lambda c: -len(c["body"]))[:2]]
    for case in json.loads((m / "recipe/eval/cases.json").read_text())["cases"]:
        if sum(map(len, parts)) >= 6500:
            break
        parts.append(case["transcript"])
    return "\n\n".join(parts)


def turn(user):
    req = urllib.request.Request(
        f"http://127.0.0.1:{PORT}/v1/chat/completions",
        data=json.dumps({
            "model": "chat-speed", "temperature": 0.2, "seed": 42, "max_tokens": 500, "stream": True,
            "messages": [{"role": "system", "content": CHAT_INSTRUCTION},
                         {"role": "user", "content": user}],
        }).encode(),
        headers={"Content-Type": "application/json"},
    )
    t0, first, text, timings = time.time(), None, "", {}
    with urllib.request.urlopen(req, timeout=600) as r:
        for raw in r:
            line = raw.decode().strip()
            if not line.startswith("data: ") or line == "data: [DONE]":
                continue
            j = json.loads(line[6:])
            timings = j.get("timings", timings)
            for ch in j.get("choices", []):
                piece = (ch.get("delta") or {}).get("content") or ""
                if piece and first is None:
                    first = time.time() - t0
                text += piece
    return text, timings, first or 0.0, time.time() - t0


NOTE = note()
ASKS = [
    "@Myrme what are the three main points of this note?",
    "@Myrme which of them is the weakest, and why?",
    "@Myrme suggest one experiment to test it.",
]
history = []
print("| turn | read tok | reused tok | first token s | reply s | written tok | write tok/s | meets bar |")
print("|---|---|---|---|---|---|---|---|")
for i, ask in enumerate(ASKS, 1):
    history.append(f"- {ask}")
    text, t, first, total = turn(f"{NOTE}\n\n" + "\n".join(history) + "\n" + ask)
    history.append(f"- {text.strip()}")
    rate = t.get("predicted_per_second") or 0.0
    meets = "yes" if first <= FIRST_WORDS_S and rate >= RATE else "no"
    print(f"| {i} | {t.get('prompt_n', '?')} | {t.get('cache_n', '?')} | {first:.2f} | {total:.2f} "
          f"| {t.get('predicted_n', '?')} | {rate:.1f} | {meets} |")
