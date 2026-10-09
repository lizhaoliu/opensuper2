import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
pal, tables = load_index()
out = '/home/lizhaoliu/games/super2sp/dump/gfx/'
e = entry('dat2', 10)
n = struct.unpack_from('<H', e, 0)[0]
for j in range(n):
    o, s = struct.unpack_from('<II', e, 2+8*j)
    r = rle_decode(e, o)
    print(j, r[0], r[1], r[3]-o, s)
    write_png(out + 'dat10_%02d.png' % j, r[0], r[1], r[2], pal, transparent=False)
