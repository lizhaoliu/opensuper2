import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
b = entry('bin', 1)
n = struct.unpack_from('<h', b, 0)[0]
def big5(x):
    return x.split(b'\0')[0].decode('big5', 'replace')
for i in range(n):
    h = b[2 + 30 * i: 2 + 30 * i + 30]
    off, size = struct.unpack_from('<II', h, 0)
    nunits = h[8]; uoff = struct.unpack_from('<I', h, 0x1a)[0]
    blk = b[off:off + size]
    p = uoff + nunits * 0x65
    hdr7 = struct.unpack_from('<7H', blk, p)
    name = blk[p + 14: p + 14 + hdr7[0]]
    print(i + 1, h[8:0x1a].hex(' '), 'uoff', uoff, 'h7', hdr7, big5(name), 'rest', size - (p + 14))
