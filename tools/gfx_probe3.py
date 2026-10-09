import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct, collections
from s2 import *
pal, tables = load_index()
for kind, idx in [('prg',2),('cel',5)]:
    e = entry(kind, idx)
    n = struct.unpack_from('<H', e, 0)[0]
    recs = [struct.unpack_from('<IHB', e, 2+7*i) for i in range(n)]
    print(kind, idx, n, recs[:6], 'flags', collections.Counter(r[2] for r in recs))
    ok = all(recs[i][0]+recs[i][1]==recs[i+1][0] for i in range(n-1))
    print(' contiguous', ok, 'last end', recs[-1][0]+recs[-1][1], len(e))
    for j in [0,1,2]:
        o,s,f = recs[j]
        print('  ', e[o:o+32].hex(' '))
idx = entry('prg',1)
n = struct.unpack_from('<H', idx, 0)[0]
r = [struct.unpack_from('<HHI', idx, 2+8*i) for i in range(n)]
print(r[:10])
names = idx[2+8*n:]
for t in range(3):
    blk = names[t*200:(t+1)*200]
    print(t, [big5(x) for x in blk.split(b'\0') if x][:30])
ci = entry('cel',4)
n = struct.unpack_from('<H', ci, 0)[0]
print('cel4', [struct.unpack_from('<HHI', ci, 2+8*i) for i in range(n)])
names = ci[2+8*n:]
for t in range(n):
    blk = names[t*200:(t+1)*200]
    print(t, [big5(x) for x in blk.split(b'\0') if x][:30])
