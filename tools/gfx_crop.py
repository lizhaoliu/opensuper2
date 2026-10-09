import os, sys, struct, zlib
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from gfx_pngmontage import read_png  # noqa
src, out, x0, y0, w, h, scale = sys.argv[1], sys.argv[2], *map(int, sys.argv[3:8])
W, H, pix, pal = read_png(src)
rgb = bytearray()
rows = []
for y in range(h):
    row = bytearray()
    for x in range(w):
        c = pix[(y0+y)*W + x0+x]
        row += pal[c*3:c*3+3] * scale
    rows += [bytes(row)] * scale
raw = b''.join(b'\0' + r for r in rows)
def chunk(t, d): return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
open(out,'wb').write(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w*scale, h*scale, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw, 6)) + chunk(b'IEND', b''))
