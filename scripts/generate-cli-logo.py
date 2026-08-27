#!/usr/bin/env python3
"""The CLI's pixel logo, generated — never hand-edited.

Renders src-tauri/icons/128x128.png as truecolor half-block ANSI art
(each terminal cell paints two vertical pixels: ▀ with a 24-bit
foreground for the top pixel and background for the bottom) into
src-tauri/src/cli_logo.ans, which cli.rs embeds at compile time.

Run from the repo root after any icon change:

    python3 scripts/generate-cli-logo.py

Requires ImageMagick (`magick`), which the icon pipeline needs anyway.
26px wide — 13 text rows — matches what basecamp-cli's welcome does and
stays modest in an 80-column terminal.
"""

import re
import subprocess
import sys

SIZE = 26
PAD = "  "  # left margin, so the logo does not hug the terminal edge
SRC = "src-tauri/icons/128x128.png"
OUT = "src-tauri/src/cli_logo.ans"

txt = subprocess.run(
    ["magick", SRC, "-resize", f"{SIZE}x{SIZE}!", "txt:-"],
    capture_output=True, text=True, check=True,
).stdout

pixels = {}
for line in txt.splitlines():
    m = re.match(r"^(\d+),(\d+): \((\d+),(\d+),(\d+)(?:,(\d+))?\)", line)
    if not m:
        continue
    x, y, r, g, b = (int(m.group(i)) for i in range(1, 6))
    a = int(m.group(6)) if m.group(6) is not None else 255
    pixels[(x, y)] = (r, g, b, a)

def solid(p):
    return p is not None and p[3] >= 128

rows = []
for y in range(0, SIZE, 2):
    cells = []
    for x in range(SIZE):
        top = pixels.get((x, y))
        bot = pixels.get((x, y + 1))
        t, b = solid(top), solid(bot)
        if not t and not b:
            cells.append(" ")
        elif t and b:
            cells.append(
                f"\x1b[38;2;{top[0]};{top[1]};{top[2]}m"
                f"\x1b[48;2;{bot[0]};{bot[1]};{bot[2]}m▀\x1b[0m"
            )
        elif t:
            cells.append(f"\x1b[38;2;{top[0]};{top[1]};{top[2]}m▀\x1b[0m")
        else:
            cells.append(f"\x1b[38;2;{bot[0]};{bot[1]};{bot[2]}m▄\x1b[0m")
    rows.append(PAD + "".join(cells).rstrip())

with open(OUT, "w") as f:
    f.write("\n".join(rows) + "\n")

print(f"wrote {OUT}: {len(rows)} rows of {SIZE} cells", file=sys.stderr)
