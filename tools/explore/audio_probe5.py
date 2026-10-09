import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
g = entry('bin', 10)
n = struct.unpack_from('<h', g, 0)[0]
p = 2 + 4 * n
m = struct.unpack_from('<h', g, p)[0]
fx = list(g[p + 2:p + 2 + m])
print('BIN10 size', len(g), 'n', n, 'm', m)
print('effect->sfx', fx)
print('rest at', p + 2 + m + 2, 'len', len(g) - (p + 2 + m + 2))
