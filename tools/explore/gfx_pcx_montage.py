import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct, zlib
from s2 import *
from gfx_pcx import pcx
ids = [int(x) for x in sys.argv[2:]]
cw, ch, cols = 320, 300, 4
rows = (len(ids)+cols-1)//cols
W, H = cw*cols, ch*rows
rgb = bytearray(W*H*3)
for j, i in enumerate(ids):
    w, h, pix, p6, *_ = pcx(entry('dat2', i))
    s = max(w/cw, h/ch)
    ox, oy = (j%cols)*cw, (j//cols)*ch
    for y in range(min(ch, int(h/s))):
        for x in range(min(cw, int(w/s))):
            c = pix[int(y*s)*w + int(x*s)]
            q = (oy+y)*W*3 + (ox+x)*3
            for k in range(3):
                rgb[q+k] = p6[c*3+k]
raw = b''.join(b'\0' + bytes(rgb[y*W*3:(y+1)*W*3]) for y in range(H))
def chunk(t, d): return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
open(sys.argv[1], 'wb').write(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', W, H, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw, 6)) + chunk(b'IEND', b''))
