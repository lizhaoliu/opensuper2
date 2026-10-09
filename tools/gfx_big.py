import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct, zlib
from s2 import *
pal, tables = load_index()
# montage of large UI images, each scaled down 4x (nearest) into a grid with labels by order
e = entry('cel',2); n = struct.unpack_from('<H', e, 0)[0]
big = []
for i in range(n):
    o,s = struct.unpack_from('<II', e, 2+8*i)
    w,h = struct.unpack_from('<hh', e, o)
    if w+1 > 64 or h+1 > 64: big.append(0x8f+i)
print(len(big), big)
