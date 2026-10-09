import struct
R = '/home/lizhaoliu/games/super2sp/dump/tables/raw/'
b = open(R + 'bin_01', 'rb').read()
k8 = open('/home/lizhaoliu/games/super2sp/extracted/app/KINDSP.BIN', 'rb').read()
def cname(c):
    o = 1355 + 37 * (c - 1); r = k8[o:o + 11]; return r[:r.index(b'\0')].decode('big5', 'replace')
import sys
st = int(sys.argv[1])
h = b[2 + 30 * (st - 1):2 + 30 * st]
off, size = struct.unpack_from('<ii', h, 0)
k = b[off:off + size]
a2 = struct.unpack_from('<h', h, 0x1a)[0]; nrec = h[8]
for r in range(nrec):
    rec = k[a2 + 101 * r:a2 + 101 * (r + 1)]
    nm = rec[2:0xf].split(b'\0')[0].decode('big5', 'replace')
    cls = struct.unpack_from('<h', rec, 0x10)[0]
    lv = struct.unpack_from('<h', rec, 0x12)[0]
    x, y = struct.unpack_from('<hh', rec, 0x17)
    stats = struct.unpack_from('<9h', rec, 0x1b)
    rest = rec[0x2d:]
    print('%2d id=%3d %-6s f=%02x cls=%3d(%s) lv=%2d b14=%3d b15=%3d b16=%3d xy=(%2d,%2d)' % (r, struct.unpack_from('<h', rec, 0)[0], nm, rec[0xf], cls, cname(cls) if cls > 0 else '-', lv, rec[0x14], rec[0x15], rec[0x16], x, y), stats, rest.hex(' '))
