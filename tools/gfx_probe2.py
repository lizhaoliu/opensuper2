import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
pal, tables = load_index()
def sub(kind, idx):
    e = entry(kind, idx)
    n = struct.unpack_from('<H', e, 0)[0]
    return e, [struct.unpack_from('<II', e, 2+8*i) for i in range(n)]
for kind, idx in [('dat2',5),('cel',2),('cel',3),('cel',5)]:
    e, t = sub(kind, idx)
    print(kind, idx, 'n=',len(t), 'first', t[:3], 'last end', hex(t[-1][0]+t[-1][1]), 'len', hex(len(e)))
    for j in [0,1,2,len(t)//2,len(t)-1]:
        o,s = t[j]
        print('   ', j, s, e[o:o+24].hex(' '))
