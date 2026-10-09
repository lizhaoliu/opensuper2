import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
def pcx(b):
    assert b[0] == 10 and b[1] == 5, b[:4]
    x0, y0, x1, y1 = struct.unpack_from('<4H', b, 4)
    bpl = struct.unpack_from('<H', b, 0x42)[0]
    w, h = x1 - x0 + 1, y1 - y0 + 1
    pal = bytes(b[-0x300:])
    data = b[0x80:-0x300]
    out = bytearray(); p = 0
    for y in range(h):
        row = bytearray()
        while len(row) < bpl:
            c = data[p]
            if c & 0xc0 == 0xc0:
                row += bytes([data[p + 1]]) * (c & 0x3f); p += 2
            else:
                row.append(c); p += 1
        out += row[:w]
    return w, h, bytes(out), pal
if __name__ == "__main__":
  for e in [int(x) for x in sys.argv[1:]]:
      b = entry('dat2', e)
      try:
          w, h, pix, pal = pcx(b)
          write_png('/home/lizhaoliu/games/super2sp/dump/flow/dat%02d.png' % e, w, h, pix, pal, transparent=False)
          print(e, w, h)
      except Exception as ex:
          print(e, 'not pcx', b[:8].hex(), len(b))
