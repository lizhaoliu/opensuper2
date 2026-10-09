import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
pal, tables = load_index()
g = entry('cel', 1)
n = struct.unpack_from('<H', g, 0)[0]
ents = [struct.unpack_from('<II', g, 2 + 8 * i) for i in range(n)]
si = int(sys.argv[1]); out = sys.argv[2]
o, s = ents[si]; e = g[o:o + s]
hdr = struct.unpack_from('<8h', e, 0)
nf, ncol, extra = hdr[0], hdr[5], hdr[6]
p = 16 + (2 * ncol if ncol else 0) + (extra if extra > 0 else 0)
ft = [struct.unpack_from('<II', e, p + 8 * i) for i in range(nf)]
data = p + 8 * nf
frames = [rle_decode(e, data + fo) for fo, fs in ft]
W = max(f[0] for f in frames); H = max(f[1] for f in frames)
cols = min(8, nf); rows = (nf + cols - 1) // cols
sheet = bytearray(W * cols * H * rows)
for i, (w, h, pix, _) in enumerate(frames):
    ox, oy = (i % cols) * W, (i // cols) * H
    for y in range(h):
        sheet[(oy + y) * W * cols + ox:(oy + y) * W * cols + ox + w] = pix[y * w:(y + 1) * w]
pal2 = bytearray(pal); pal2[0:3] = b'\x20\x20\x40'
write_png(out, W * cols, H * rows, bytes(sheet), bytes(pal2), transparent=False)
print(nf, W, H)
