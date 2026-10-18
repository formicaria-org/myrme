"""Fetch the openly licensed eval data a method's cases point at, into .eval-data/ (gitignored).

    pixi run eval-data -- speech            # builds methods/audio-text/eval/cases.json + the audio

Every source is pinned by revision and every derived file is checked by sha256, so a case file
committed here always means the same clip. Only the case file (ids, checksums, reference text) is
committed; the audio stays in the cache. Sources and licences: docs/research/2026-10-18-eval-data-
sources.md, and the attribution in methods/<name>/eval/NOTICE.md.

Run with `python -I` (pixi does): the datasets are untrusted input and are only ever parsed.
"""

import hashlib
import json
import pathlib
import subprocess
import sys
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parent.parent
CACHE = ROOT / ".eval-data"

# One pinned file per language: (dataset, revision, file, transcript column, licence).
SPEECH = {
    "it": (
        "facebook/multilingual_librispeech",
        "2e83e61823b4c47dcbcb1980bb88601274127609",
        "italian/test-00000-of-00001.parquet",
        "transcript",
        "CC-BY-4.0",
    ),
    "en": (
        "openslr/librispeech_asr",
        "71cacbfb7e2354c4226d01e70d77d5fca3d04ba1",
        "all/test.clean/0000.parquet",
        "text",
        "CC-BY-4.0",
    ),
}
PER_LANGUAGE = 30


def fetch(dataset: str, revision: str, file: str) -> pathlib.Path:
    dest = CACHE / "cache" / dataset.replace("/", "__") / revision / file
    if not dest.exists():
        dest.parent.mkdir(parents=True, exist_ok=True)
        url = f"https://huggingface.co/datasets/{dataset}/resolve/{revision}/{file}"
        print(f"fetching {url}", file=sys.stderr)
        tmp = dest.with_suffix(".part")
        urllib.request.urlretrieve(url, tmp)
        tmp.rename(dest)
    return dest


def to_wav(raw: bytes, dest: pathlib.Path) -> str:
    """Decode any audio ffmpeg reads into 16 kHz mono 16-bit WAV, the form whisper takes."""
    dest.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["ffmpeg", "-loglevel", "error", "-y", "-i", "pipe:0", "-ar", "16000", "-ac", "1",
         "-c:a", "pcm_s16le", str(dest)],
        input=raw, check=True,
    )
    return hashlib.sha256(dest.read_bytes()).hexdigest()


def speech() -> None:
    import pyarrow.parquet as pq

    cases = []
    for lang, (dataset, revision, file, column, licence) in SPEECH.items():
        table = pq.read_table(fetch(dataset, revision, file), columns=["audio", column])
        n = table.num_rows
        # Spread across the file, so the clips are many speakers rather than one chapter.
        rows = [round(i * (n - 1) / (PER_LANGUAGE - 1)) for i in range(PER_LANGUAGE)]
        for row in rows:
            r = table.slice(row, 1).to_pylist()[0]
            cid = f"{lang}-{row:04d}"
            sha = to_wav(r["audio"]["bytes"], CACHE / "audio-text" / f"{cid}.wav")
            cases.append({
                "id": cid,
                "lang": lang,
                "source": {"dataset": dataset, "revision": revision, "file": file, "row": row,
                           "licence": licence},
                "wav_sha256": sha,
                "reference": r[column].strip(),
            })
    out = ROOT / "methods" / "audio-text" / "eval" / "cases.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    old = json.loads(out.read_text()) if out.exists() else {}
    recorded = {c["id"]: c.get("recorded", {}) for c in old.get("cases", [])}
    for c in cases:
        if recorded.get(c["id"]):
            c["recorded"] = recorded[c["id"]]
    doc = {
        "about": "Golden cases for M4, speech to text: read speech in Italian (Multilingual "
                 "LibriSpeech) and English (LibriSpeech test-clean), 30 clips each spread over the "
                 "test file, both CC-BY-4.0 (see NOTICE.md). The audio is not committed: "
                 "`pixi run eval-data -- speech` rebuilds it into .eval-data/ from the pinned files "
                 "and checks each clip against `wav_sha256`. Scored by word error rate against "
                 "`reference` after normalisation (`fm_agent::eval::wer`). `recorded` holds real "
                 "readers' transcripts.",
        "cases": cases,
    }
    out.write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n")
    print(f"{len(cases)} cases -> {out.relative_to(ROOT)}", file=sys.stderr)


def check_speech() -> None:
    """Rebuild missing audio and fail on any clip whose bytes differ from the case file."""
    cases = json.loads((ROOT / "methods/audio-text/eval/cases.json").read_text())["cases"]
    wav = CACHE / "audio-text"
    if any(not (wav / f"{c['id']}.wav").exists() for c in cases):
        speech()
    for c in cases:
        got = hashlib.sha256((wav / f"{c['id']}.wav").read_bytes()).hexdigest()
        if got != c["wav_sha256"]:
            sys.exit(f"{c['id']}: audio differs from the case file ({got})")


# Wikibooks Cookbook (CC BY-SA 4.0): recipe pages, parsed for their Ingredients and Procedure.
RECIPES = ("gossminn/wikibooks-cookbook", "49fb73ea07d2a1380c15197db5589934937457ed")
RECIPE_COUNT = 30


def recipe_page(html: str):
    """(title, ingredients, steps) from a Wikibooks Cookbook page, or None when it has not both."""
    import html as htmlmod
    import re

    title = re.search(r"<title>Cookbook:(.*?) - Wikibooks", html)
    def section(anchor: str, tag: str):
        i = html.find(f'id="{anchor}"')
        if i < 0:
            return []
        end = html.find("<h2", i + 1)
        block = html[i:end if end > 0 else len(html)]
        m = re.search(rf"<{tag}[^>]*>(.*?)</{tag}>", block, re.S)
        if not m:
            return []
        items = re.findall(r"<li[^>]*>(.*?)</li>", m.group(1), re.S)
        clean = lambda t: re.sub(r"\s+", " ", htmlmod.unescape(re.sub(r"<[^>]+>", "", t))).strip()
        return [c for c in (clean(x) for x in items) if c]
    if not title:
        return None
    ing, steps = section("Ingredients", "ul"), section("Procedure", "ol")
    if not (3 <= len(ing) <= 14 and 2 <= len(steps) <= 10):
        return None
    if any(len(x) > 220 for x in ing + steps):
        return None
    return title.group(1).strip(), ing, steps


def spoken(title: str, ing, steps) -> str:
    """The recipe as someone might say it into a phone: one run of speech, no lists."""
    lower = lambda t: t[:1].lower() + t[1:]
    said = f"Okay, so today I'm making {title}. For this you need {', '.join(ing[:-1])} and {ing[-1]}. "
    links = ["First", "Then", "After that", "Next", "Then", "After that", "Next", "Then", "After that", "Finally"]
    for n, step in enumerate(steps):
        link = "Finally" if n == len(steps) - 1 and n > 0 else links[n]
        said += f"{link}, {lower(step.rstrip('.'))}. "
    return said + "And that's it."


def recipes() -> None:
    dataset, revision = RECIPES
    cases = []
    i = 0
    while len(cases) < RECIPE_COUNT and i < 400:
        file = f"recipes/recipe_00_{i:03d}.html"
        i += 1
        try:
            page = recipe_page(fetch(dataset, revision, file).read_text(errors="replace"))
        except Exception as e:  # a missing page number is skipped, not fatal
            print(f"skip {file}: {e}", file=sys.stderr)
            continue
        if not page:
            continue
        title, ing, steps = page
        cases.append({
            "id": f"wb-{i - 1:03d}",
            "source": {"dataset": dataset, "revision": revision, "file": file},
            "title": title,
            "transcript": spoken(title, ing, steps),
            "ingredients": ing,
            "steps": steps,
        })
    out = ROOT / "methods" / "recipe" / "eval" / "cases.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    old = json.loads(out.read_text()) if out.exists() else {}
    recorded = {c["id"]: c.get("recorded", {}) for c in old.get("cases", [])}
    for c in cases:
        if recorded.get(c["id"]):
            c["recorded"] = recorded[c["id"]]
    doc = {
        "about": "Golden cases for M7, a recipe from what was said. Each is a Wikibooks Cookbook "
                 "recipe (CC BY-SA 4.0, see LICENSE.md here), its Ingredients and Procedure taken "
                 "as the ground truth, and turned by `agents/eval-data.py` into one run of speech "
                 "(`transcript`) — the input the recipe method gets after speech-to-text, which is "
                 "M4's job. Scored by `fm_agent::eval::recipe_outcome`: every listed ingredient "
                 "found, none invented, and enough steps.",
        "cases": cases,
    }
    out.write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n")
    print(f"{len(cases)} cases -> {out.relative_to(ROOT)}", file=sys.stderr)


# GNHK (GoodNotes, CC BY 4.0): phone photographs of handwritten pages, word boxes with line numbers.
GNHK = ("aryaman27/goodnotes-handwritten-dataset", "904297760a9fec813fdc1ecb5f8055cb1ca03550")
GNHK_COUNT = 30


def handwriting() -> None:
    dataset, revision = GNHK
    listing = json.loads(urllib.request.urlopen(
        f"https://huggingface.co/api/datasets/{dataset}/tree/{revision}/test_data/test").read())
    stems = sorted(f["path"][:-4] for f in listing if f["path"].endswith(".jpg"))
    picks = [stems[round(i * (len(stems) - 1) / (GNHK_COUNT - 1))] for i in range(GNHK_COUNT)]
    cases = []
    for stem in picks:
        words = json.loads(fetch(dataset, revision, stem + ".json").read_text())
        jpg = fetch(dataset, revision, stem + ".jpg")
        # Lines in order, words left to right; GNHK's %…% tokens (math, special signs, illegible)
        # say there is something a transcript cannot be held to, and are left out.
        lines: dict = {}
        for w in words:
            if w["text"].startswith("%") and w["text"].endswith("%"):
                continue
            x = sum(w["polygon"][f"x{i}"] for i in range(4)) / 4
            lines.setdefault(w["line_idx"], []).append((x, w["text"]))
        reference = "\n".join(" ".join(t for _, t in sorted(ws)) for _, ws in sorted(lines.items()))
        cid = stem.rsplit("/", 1)[-1]
        dest = CACHE / "image-text" / f"{cid}.jpg"
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(jpg.read_bytes())
        cases.append({
            "id": cid,
            "source": {"dataset": dataset, "revision": revision, "file": stem + ".jpg",
                       "licence": "CC-BY-4.0"},
            "jpg_sha256": hashlib.sha256(dest.read_bytes()).hexdigest(),
            "reference": reference,
        })
    out = ROOT / "methods" / "image-text" / "eval" / "cases.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    old = json.loads(out.read_text()) if out.exists() else {}
    recorded = {c["id"]: c.get("recorded", {}) for c in old.get("cases", [])}
    for c in cases:
        if recorded.get(c["id"]):
            c["recorded"] = recorded[c["id"]]
    doc = {
        "about": "Golden cases for M3, image to text: 30 phone photographs of handwritten pages from "
                 "GNHK's test split (GoodNotes, CC BY 4.0; see ../NOTICE.md), spread over it. The "
                 "reference is GNHK's own word annotation, line by line, with its %…% tokens (math, "
                 "signs, illegible) left out. The images are not committed: `pixi run eval-data -- "
                 "handwriting` fetches them and checks `jpg_sha256`. Scored by word error rate "
                 "(`fm_agent::eval::wer`).",
        "cases": cases,
    }
    out.write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n")
    print(f"{len(cases)} cases -> {out.relative_to(ROOT)}", file=sys.stderr)


if __name__ == "__main__":
    {"speech": speech, "check-speech": check_speech, "recipes": recipes, "handwriting": handwriting}[sys.argv[1]]()
