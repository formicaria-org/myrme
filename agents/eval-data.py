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


if __name__ == "__main__":
    {"speech": speech, "check-speech": check_speech}[sys.argv[1]]()
