import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
pal, tables = load_index()
def pcx(e):
    assert e[0] == 10 and e[1] == 5 and e[2] == 1, e[:4].hex()
    x0, y0, x1, y1 = struct.unpack_from('<4H', e, 4)
    planes, bpl = e[0x41], struct.unpack_from('<H', e, 0x42)[0]
    w, h = x1-x0+1, y1-y0+1
    data = e[128:len(e)-0x300]
    p6 = e[len(e)-0x300:]
    out = bytearray(); p = 0
    for y in range(h):
        row = bytearray()
        while len(row) < bpl:
            c = data[p]; p += 1
            if c & 0xc0 == 0xc0:
                row += bytes([data[p]]) * (c & 0x3f); p += 1
            else: row.append(c)
        out += row[:w]
    return w, h, bytes(out), bytes(p6), planes, bpl, p, len(data)
if __name__ == "__main__":
  for i in list(range(11, 21)) + list(range(26, 41)):
      e = entry("dat2", i)
      w, h, pix, p8, planes, bpl, used, dl = pcx(e)
      print(i, w, h, planes, bpl, 'used', used, 'of', dl, 'marker', hex(e[128+used]) if 128+used < len(e) else None)
      write_png('/home/lizhaoliu/games/super2sp/dump/gfx/pcx_%02d.png' % i, w, h, pix, p8, transparent=False)
