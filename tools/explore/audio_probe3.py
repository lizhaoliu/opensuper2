import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
g = entry('bin', 7)
n = struct.unpack_from('<h', g, 0)[0]
print('BIN7 size', len(g), 'records', n)
for i in range(n):
    r = g[2 + 21 * i: 2 + 21 * (i + 1)]
    off = struct.unpack_from('<I', r, 0)[0]
    cnt = struct.unpack_from('<h', r, 0x13)[0]
    cues = [struct.unpack_from('<3h', g, off + 6 * k) for k in range(cnt)]
    print(i + 1, r.hex(' '), 'cues', cues[:12], '...' if cnt > 12 else '')
