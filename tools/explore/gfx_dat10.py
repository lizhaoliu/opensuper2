import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
pal, tables = load_index()
out = '/home/lizhaoliu/games/super2sp/dump/gfx/'
e = entry('dat2', 7)
n = struct.unpack_from('<H', e, 0)[0]
for j in range(n):
    o, s = struct.unpack_from('<II', e, 2+8*j)
    w, h, pix, used = flagged(e, o)
    print('dat7', j, w, h, used, s)
    write_png(out + 'dat7_%02d.png' % j, w, h, pix, pal, transparent=False)
e = entry('dat2', 10)
n = struct.unpack_from('<H', e, 0)[0]
t = [struct.unpack_from('<II', e, 2+8*i) for i in range(n)]
print('dat10', n, t[-1][0]+t[-1][1], len(e))
for j,(o,s) in enumerate(t):
    print(j, s, e[o:o+12].hex(' '))
