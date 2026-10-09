import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
e = entry('prg', 8)
n = struct.unpack_from('<H', e, 0)[0]
t = [struct.unpack_from('<II', e, 2+8*i) for i in range(n)]
print(n, t[:6], t[-1][0]+t[-1][1], len(e))
for j in range(n):
    o, s = t[j]
    print(j, s, e[o:o+16].hex(' '))
