import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct, collections
from s2 import *
g = entry('bin', 1)
n = struct.unpack_from('<h', g, 0)[0]
print('BIN1 size', len(g), 'stages', n)
mus = []
for i in range(n):
    r = g[2 + 30 * i: 32 + 30 * i]
    mus.append(r[10])
    if i < 6 or i > n - 3: print(i + 1, r.hex(' '))
print('music per stage:', mus)
print(collections.Counter(mus).most_common())
f = open(D + 'FSTAGE.BIN', 'rb').read()
m = struct.unpack_from('<h', f, 0)[0]
print('FSTAGE stages', m, [f[2 + 30 * i + 10] for i in range(m)])
