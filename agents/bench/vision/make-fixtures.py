#!/usr/bin/env python3
"""Synthetic ground-truth images for the transcription pipeline.

Deliberately *typeset*, not handwritten: the point of this set is to test the DATASET pipeline, and
a fixture whose ground truth is ambiguous tests the reader instead. Handwriting is the harder,
real-world case and belongs in a separate set where disagreement with the ground truth is the
finding rather than the noise.

Each image ships with the transcript a correct reading should produce, so the same fixtures can
later score a real model rather than only exercising the plumbing.
"""
import json, os
from PIL import Image, ImageDraw, ImageFont

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "fixtures")
os.makedirs(OUT, exist_ok=True)
FONT = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"
MONO = "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"
SERIF = "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf"

def canvas(w=900, h=520):
    im = Image.new("RGB", (w, h), "white")
    return im, ImageDraw.Draw(im)

def lines(d, xy, rows, font, fill="black", lead=42):
    x, y = xy
    for r in rows:
        d.text((x, y), r, font=font, fill=fill)
        y += lead

cases = []

# 1 — plain prose notes
im, d = canvas(900, 320)
f = ImageFont.truetype(FONT, 30)
lines(d, (40, 40), [
    "Meeting notes - battery review",
    "Cell chemistry chosen: LiFePO4",
    "Cycle target: 3000 cycles at 80% DoD",
    "Open question: cold weather performance",
], f)
im.save(f"{OUT}/01-notes.png")
cases.append({
    "file": "01-notes.png", "kind": "prose",
    "truth": "Meeting notes - battery review\nCell chemistry chosen: LiFePO4\n"
             "Cycle target: 3000 cycles at 80% DoD\nOpen question: cold weather performance",
})

# 2 — mathematics, which must come back as LaTeX
im, d = canvas(900, 300)
fs = ImageFont.truetype(SERIF, 34)
lines(d, (40, 40), [
    "Energy stored in a capacitor:",
    "E = 1/2 C V^2",
    "For C = 4.7 uF and V = 12 V:",
    "E = 0.5 * 4.7e-6 * 144 = 3.38e-4 J",
], fs, lead=54)
im.save(f"{OUT}/02-math.png")
cases.append({
    "file": "02-math.png", "kind": "math",
    "truth": "Energy stored in a capacitor:\nE = 1/2 C V^2\nFor C = 4.7 uF and V = 12 V:\n"
             "E = 0.5 * 4.7e-6 * 144 = 3.38e-4 J",
    "expect": ["latex-or-plain-formula"],
})

# 3 — pseudocode, which must come back fenced and indented
im, d = canvas(900, 340)
fm = ImageFont.truetype(MONO, 26)
lines(d, (40, 36), [
    "function charge(cell, target):",
    "    while cell.voltage < target:",
    "        apply_current(0.5)",
    "        if cell.temp > 45:",
    "            break",
    "    return cell.voltage",
], fm, lead=48)
im.save(f"{OUT}/03-pseudocode.png")
cases.append({
    "file": "03-pseudocode.png", "kind": "code",
    "truth": "function charge(cell, target):\n    while cell.voltage < target:\n"
             "        apply_current(0.5)\n        if cell.temp > 45:\n            break\n"
             "    return cell.voltage",
})

# 4 — a table
im, d = canvas(900, 300)
f = ImageFont.truetype(MONO, 26)
rows = [
    "Chemistry   Wh/kg   Cycles",
    "LiFePO4      160     3000",
    "NMC          230     1500",
    "LTO           80    15000",
]
lines(d, (40, 44), rows, f, lead=48)
d.line((36, 84, 560, 84), fill="black", width=2)
im.save(f"{OUT}/04-table.png")
cases.append({
    "file": "04-table.png", "kind": "table",
    "truth": "| Chemistry | Wh/kg | Cycles |\n|---|---|---|\n| LiFePO4 | 160 | 3000 |\n"
             "| NMC | 230 | 1500 |\n| LTO | 80 | 15000 |",
})

# 5 — a labelled plot: the labels are the transcribable part
im, d = canvas(760, 560)
f = ImageFont.truetype(FONT, 24)
fb = ImageFont.truetype(FONT, 28)
d.text((210, 24), "Capacity fade vs cycles", font=fb, fill="black")
d.line((110, 470, 700, 470), fill="black", width=3)   # x axis
d.line((110, 470, 110, 90), fill="black", width=3)    # y axis
d.text((330, 500), "Cycle count", font=f, fill="black")
im2 = Image.new("RGB", (220, 34), "white")
ImageDraw.Draw(im2).text((0, 0), "Capacity (%)", font=f, fill="black")
im.paste(im2.rotate(90, expand=True), (30, 200))
for i, lab in enumerate(["0", "1000", "2000", "3000"]):
    d.text((100 + i * 190, 478), lab, font=f, fill="black")
for i, lab in enumerate(["100", "90", "80"]):
    d.text((60, 100 + i * 130), lab, font=f, fill="black")
pts = [(110, 110), (300, 150), (490, 215), (680, 320)]
d.line(pts, fill="black", width=4)
for p in pts:
    d.ellipse((p[0] - 6, p[1] - 6, p[0] + 6, p[1] + 6), fill="black")
d.text((420, 150), "LiFePO4", font=f, fill="black")
im.save(f"{OUT}/05-plot.png")
cases.append({
    "file": "05-plot.png", "kind": "figure",
    "truth": "Title: Capacity fade vs cycles\nX axis: Cycle count (0, 1000, 2000, 3000)\n"
             "Y axis: Capacity (%) (100, 90, 80)\nSeries label: LiFePO4",
    "expect": ["labels-transcribed", "one-line-naming-the-figure"],
})

with open(f"{OUT}/ground-truth.json", "w") as fh:
    json.dump(cases, fh, indent=2)
print(f"wrote {len(cases)} fixtures to {OUT}")
for c in cases:
    p = f"{OUT}/{c['file']}"
    print(f"  {c['file']:20} {c['kind']:10} {os.path.getsize(p):>7} bytes")
