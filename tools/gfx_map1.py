import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct, collections
from s2 import *
pal, tables = load_index()
for k in (3,4,5):
    e = entry('prg',k); n = struct.unpack_from('<H', e, 0)[0]
    t = [struct.unpack_from('<II', e, 2+8*i) for i in range(n)]
    print('prg',k,'n',n,t[:3], 'end', t[-1][0]+t[-1][1], len(e))
    for j in (0,1,n//2):
        o,s = t[j]; print('   ', s, e[o:o+20].hex(' '))
        try:
            fr = rle_decode(e,o); print('    rle', fr[0], fr[1], fr[3]-o)
        except Exception as ex: print('    no', ex)
e = entry('prg',7)
print('prg7', struct.unpack_from('<3h', e, 0), len(e))
e = entry('prg',6); n = struct.unpack_from('<H', e, 0)[0]
t = [struct.unpack_from('<II', e, 2+8*i) for i in range(n)]
print('prg6 n', n, t[:4])
for j in (0,1,2):
    o,s = t[j]
    h = struct.unpack_from('<14h', e, o)
    print('map', j, s, h)
