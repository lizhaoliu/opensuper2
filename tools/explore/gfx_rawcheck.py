import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct, collections
from s2 import *
for k in (3,4,5):
    e = entry('prg',k); n = struct.unpack_from('<H', e, 0)[0]
    c = collections.Counter(); bad = []
    for i in range(n):
        o,s = struct.unpack_from('<II', e, 2+8*i)
        w,h = struct.unpack_from('<hh', e, o)
        c[(w+1,h+1)] += 1
        if s != (w+1)*(h+1)+4: bad.append((i,s,w+1,h+1))
    print(k, n, c.most_common(8), 'bad', bad[:5], len(bad))
e = entry('prg',7)
n3,n4,n5 = struct.unpack_from('<3h', e, 0)
a3 = struct.unpack_from('<%dH'%n3, e, 6); a4 = struct.unpack_from('<%dH'%n4, e, 6+2*n3); a5 = struct.unpack_from('<%dH'%n5, e, 6+2*n3+2*n4)
print('prg7 hi bytes 3', collections.Counter(x>>8 for x in a3).most_common(12), 'lo', collections.Counter(x&0xff for x in a3).most_common(12))
print('prg7 hi bytes 4', collections.Counter(x>>8 for x in a4).most_common(20))
print('prg7 5', collections.Counter(a5).most_common(10))
print(len(e), 6+2*(n3+n4+n5))
