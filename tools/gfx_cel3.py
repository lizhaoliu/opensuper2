import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct, collections
from s2 import *

pal, tables = load_index()
e = entry('cel',3)
n = struct.unpack_from('<H', e, 0)[0]
t = [struct.unpack_from('<II', e, 2+8*i) for i in range(n)]
flags = collections.Counter()
frames=[]
for i,(o,s) in enumerate(t):
    fl = e[o]
    flags[(i%2, fl)] += 1
    if fl == 0:
        fr = rle_decode(e, o+1)
        assert fr[3]-o == s, (i, fr[3]-o, s)
    else:
        w,h = struct.unpack_from('<hh', e, o+1); w+=1; h+=1
        assert 5+w*h == s, (i, s, w, h)
        fr = (w,h,e[o+5:o+5+w*h],0)
    frames.append(fr)
print(flags)
print(collections.Counter((f[0],f[1]) for f in frames[0::2]), collections.Counter((f[0],f[1]) for f in frames[1::2]))
sheet(frames[:48], '/home/lizhaoliu/games/super2sp/dump/gfx/cel3_0_47.png', pal, cols=12)
sheet(frames[160:208], '/home/lizhaoliu/games/super2sp/dump/gfx/cel3_160.png', pal, cols=12)
