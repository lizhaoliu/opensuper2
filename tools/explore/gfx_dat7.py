import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
pal, tables = load_index()
for idx in (6, 7, 8, 9, 10):
    e = entry('dat2', idx)
    print('DAT', idx, len(e), e[:40].hex(' '))
e = entry('dat2', 7)
n = struct.unpack_from('<H', e, 0)[0]
t = [struct.unpack_from('<II', e, 2+8*i) for i in range(n)]
print('dat7 n', n, t, t[-1][0]+t[-1][1], len(e))
fr = []
for j,(o,s) in enumerate(t):
    print(j, s, e[o:o+12].hex(' '))
    w,h = struct.unpack_from('<hh', e, o)
    if s == (w+1)*(h+1)+4: fr.append((w+1,h+1,e[o+4:o+s])); print('   raw', w+1, h+1)
    else:
        try:
            f = rle_decode(e, o); print('   rle', f[0], f[1], f[3]-o); fr.append(f)
        except Exception as ex: print('   ?', ex)
