import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
out = '/home/lizhaoliu/games/super2sp/dump/gfx/'
t19 = entry('dat2', 19); pal19 = t19[-768:]
e = entry('dat2', 10)
n = struct.unpack_from('<H', e, 0)[0]
for j in range(n):
    o, s = struct.unpack_from('<II', e, 2+8*j)
    r = rle_decode(e, o)
    write_png(out + 'dat10t_%02d.png' % j, r[0], r[1], r[2], pal19, transparent=False)
e = entry('dat2', 7)
for j in (6, 10):
    o, s = struct.unpack_from('<II', e, 2+8*j)
    w, h, pix, used = flagged(e, o)
    write_png(out + 'dat7t_%02d.png' % j, w, h, pix, pal19, transparent=False)
