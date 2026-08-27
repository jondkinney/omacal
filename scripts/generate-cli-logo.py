#!/usr/bin/env python3
"""The CLI's pixel logo, generated — never hand-edited.

Renders src-tauri/icons/128x128.png as truecolor half-block ANSI art
(each terminal cell paints two vertical pixels: ▀ with a 24-bit
foreground for the top pixel and background for the bottom) into
src-tauri/src/cli_logo.ans, which cli.rs embeds at compile time.

Run from the repo root after any icon change:

    python3 scripts/generate-cli-logo.py

Requires ImageMagick (`magick`), which the icon pipeline needs anyway.
36px wide — 18 text rows — stays modest in an 80-column terminal.
Edge pixels composite over black rather than snapping to solid or
transparent, so the rounded corners anti-alias on dark terminals
instead of stair-stepping.
"""

import re
import subprocess
import sys

SIZE = 36
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
    x, y = int(m.group(1)), int(m.group(2))
    # Lanczos overshoots on sharp edges; magick emits the raw >255 values.
    r, g, b = (min(255, int(m.group(i))) for i in (3, 4, 5))
    a = min(255, int(m.group(6))) if m.group(6) is not None else 255
    pixels[(x, y)] = (r, g, b, a)

def shade(p):
    """Composite over black; None when effectively transparent."""
    if p is None or p[3] < 64:
        return None
    r, g, b, a = p
    return (r * a // 255, g * a // 255, b * a // 255)

rows = []
for y in range(0, SIZE, 2):
    cells = []
    for x in range(SIZE):
        t = shade(pixels.get((x, y)))
        b = shade(pixels.get((x, y + 1)))
        if t is None and b is None:
            cells.append(" ")
        elif t is not None and b is not None:
            cells.append(
                f"\x1b[38;2;{t[0]};{t[1]};{t[2]}m"
                f"\x1b[48;2;{b[0]};{b[1]};{b[2]}m▀\x1b[0m"
            )
        elif t is not None:
            cells.append(f"\x1b[38;2;{t[0]};{t[1]};{t[2]}m▀\x1b[0m")
        else:
            cells.append(f"\x1b[38;2;{b[0]};{b[1]};{b[2]}m▄\x1b[0m")
    rows.append(PAD + "".join(cells).rstrip())

with open(OUT, "w") as f:
    f.write("\n".join(rows) + "\n")

print(f"wrote {OUT}: {len(rows)} rows of {SIZE} cells", file=sys.stderr)
