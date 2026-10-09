import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct, sys
from s2 import *
pal, tables = load_index()
g = entry('cel', 1)
n = struct.unpack_from('<H', g, 0)[0]
ents = [struct.unpack_from('<II', g, 2 + 8 * i) for i in range(n)]
out = '/home/lizhaoliu/games/super2sp/dump/'
for si in [0, 1, 50, 100]:
    o, s = ents[si]; e = g[o:o + s]
    hdr = struct.unpack_from('<8h', e, 0)
    nf, ncol, extra = hdr[0], hdr[5], hdr[6]
    p = 16 + (2 * ncol if ncol else 0) + (extra if extra > 0 else 0)
    ft = [struct.unpack_from('<II', e, p + 8 * i) for i in range(nf)]
    data = p + 8 * nf
    print(si, hdr, ft[:3])
    fo, fs = ft[0]
    w, h, pix, end = rle_decode(e, data + fo)
    print('  frame0', w, h, 'consumed', end - (data + fo), 'of', fs)
    write_png(out + 'cel1_%03d_f0.png' % si, w, h, pix, pal)
