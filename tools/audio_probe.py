import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct, collections
from s2 import *
for eid in (17, 18):
    g = entry('bin', eid)
    n = struct.unpack_from('<H', g, 0)[0]
    ents = [struct.unpack_from('<II', g, 2 + 8 * i) for i in range(n)]
    print('BIN', eid, 'size', len(g), 'n', n, 'table end', 2 + 8 * n, 'first off', ents[0][0])
    heads = collections.Counter(g[o:o + 4] for o, s in ents)
    print('  heads', heads.most_common(6))
    print('  last end', ents[-1][0] + ents[-1][1])
    for i in [0, 1, n - 1]:
        o, s = ents[i]
        print('  ', i + 1, o, s, g[o:o + 48].hex(' '))
