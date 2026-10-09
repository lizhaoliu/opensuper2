"""Compose sysimg PNG-like raw data into a labelled-by-position grid (crop to cell)."""
import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
pal, _ = load_index()
def table(b):
    n = struct.unpack_from('<H', b, 0)[0]
    return [struct.unpack_from('<II', b, 2 + 8 * i) for i in range(n)]
imgs = []
d5 = entry('dat2', 5)
for o, s in table(d5): imgs.append(d5[o:o + s])
c2 = entry('cel', 2)
for o, s in table(c2)[:58]: imgs.append(c2[o:o + s])
def dec(b):
    w, h = struct.unpack_from('<hh', b, 0); w += 1; h += 1
    if len(b) == w * h + 4: return w, h, b[4:]
    w, h, pix, _ = rle_decode(b); return w, h, pix
ids = [int(x) for x in sys.argv[2:]]
C = int(sys.argv[1]); cols = 10
rows = (len(ids) + cols - 1) // cols
W, H = cols * C, rows * C
canvas = bytearray([1] * (W * H))
for k, i in enumerate(ids):
    w, h, pix = dec(imgs[i])
    ox, oy = (k % cols) * C, (k // cols) * C
    for y in range(min(h, C - 2)):
        for x in range(min(w, C - 2)):
            c = pix[y * w + x]
            canvas[(oy + y) * W + ox + x] = c if c else 0
p2 = bytearray(pal); p2[0:3] = b'\x40\x40\x60'; p2[3:6] = b'\xff\xff\xff'
write_png('/home/lizhaoliu/games/super2sp/dump/flow/sheet_%s.png' % (sys.argv[2] + '_' + sys.argv[-1]), W, H, bytes(canvas), bytes(p2), transparent=False)
