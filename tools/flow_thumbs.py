"""Downscale several PCX DAT entries 4x into one sheet: flow_thumbs.py out.png id..."""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from s2 import *
from flow_pcx import pcx
ids = [int(x) for x in sys.argv[2:]]
S = 4; cw, ch = 320, 240
cols = 3; rows = (len(ids) + cols - 1) // cols
W, H = cw * cols, ch * rows
# true-color thumbnails: write as RGB PNG
import zlib, struct
rgb = bytearray(W * H * 3)
for k, e in enumerate(ids):
    w, h, pix, pal = pcx(entry('dat2', e))
    sx = max(1, (w + cw - 1) // cw); sy = max(1, (h + ch - 1) // ch); s = max(sx, sy)
    ox, oy = (k % cols) * cw, (k // cols) * ch
    for y in range(min(ch, h // s)):
        for x in range(min(cw, w // s)):
            c = pix[(y * s) * w + x * s]
            o = ((oy + y) * W + ox + x) * 3
            rgb[o:o + 3] = pal[c * 3:c * 3 + 3]
raw = b''.join(b'\0' + bytes(rgb[y * W * 3:(y + 1) * W * 3]) for y in range(H))
def chunk(t, d): return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
open(sys.argv[1], 'wb').write(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', W, H, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b''))
