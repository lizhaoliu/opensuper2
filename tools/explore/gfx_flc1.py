import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
pal, tables = load_index()
def fix_header(h):
    h = bytearray(h)
    a = h[0:4]; h[0:4] = h[0x50:0x54]; h[0x50:0x54] = a
    a = h[6:8]; h[6:8] = h[0xe:0x10]; h[0xe:0x10] = a
    for off, d in ((4, 0x923), (8, 0x38), (10, 0xb), (12, 8)):
        v = (struct.unpack_from('<H', h, off)[0] - d) & 0xffff
        struct.pack_into('<H', h, off, v)
    return bytes(h)
for i, (o, s) in enumerate(tables['dat2']):
    e = entry('dat2', i+1)
    h = fix_header(e[:128])
    size, magic, frames, w, hh, depth, flags, speed = struct.unpack_from('<IHHHHHHI', h, 0)
    of1, of2 = struct.unpack_from('<II', h, 0x50)
    print(i+1, s, hex(magic), 'frames', frames, w, hh, depth, 'flags', flags, 'speed', speed, 'size', size, 'of1', of1, of2, '| raw', e[:8].hex())
