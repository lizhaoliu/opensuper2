import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct, collections
from s2 import *
pal, tables = load_index()
allf = []
for kind, idx in [('dat2',5),('cel',2)]:
    e = entry(kind, idx)
    n = struct.unpack_from('<H', e, 0)[0]
    t = [struct.unpack_from('<II', e, 2+8*i) for i in range(n)]
    bad = 0
    for i,(o,s) in enumerate(t):
        try:
            fr = rle_decode(e, o)
            if fr[3]-o != s: bad += 1; print(kind, i, 'consumed', fr[3]-o, 'size', s, (fr[0], fr[1]))
            allf.append(fr)
        except Exception as ex:
            bad += 1; print(kind, i, 'ERR', ex, e[o:o+8].hex())
            allf.append((1,1,b'\0',0))
    print(kind, idx, n, 'bad', bad)
sizes = [(i+1, f[0], f[1]) for i,f in enumerate(allf)]
for i in range(0, len(sizes), 1):
    pass
print(sizes[:150])
print(sizes[140:300])
