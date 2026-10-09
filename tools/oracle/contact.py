#!/usr/bin/env python3
"""Tile screenshots (RGB or indexed PNG) at 1/4 scale into one sheet: contact.py OUT.png IN.png..."""
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from compare import read_png, write_png  # noqa: E402


def main():
    out, files = sys.argv[1], sys.argv[2:]
    cols, s = 4, 4
    cw, ch = 640 // s, 480 // s
    rows = (len(files) + cols - 1) // cols
    w, h = cw * cols, ch * rows
    rgb = bytearray(w * h * 3)
    for k, f in enumerate(files):
        iw, _, px = read_png(f)
        ox, oy = (k % cols) * cw, (k // cols) * ch
        for y in range(ch):
            for x in range(cw):
                o = ((oy + y) * w + ox + x) * 3
                rgb[o:o + 3] = bytes(px[(y * s) * iw + x * s])
    write_png(out, w, h, bytes(rgb))


if __name__ == "__main__":
    main()
