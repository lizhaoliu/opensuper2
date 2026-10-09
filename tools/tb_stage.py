import struct
R = '/home/lizhaoliu/games/super2sp/dump/tables/raw/'
b = open(R + 'bin_01', 'rb').read()
n = struct.unpack_from('<h', b, 0)[0]
print('stages', n)
for i in range(n):
    h = b[2 + 30 * i:2 + 30 * i + 30]
    off, size = struct.unpack_from('<ii', h, 0)
    print('%3d off=%7d size=%6d' % (i + 1, off, size), h[8:].hex(' '))
def blk(i):
    h = b[2 + 30 * (i - 1):2 + 30 * i]
    off, size = struct.unpack_from('<ii', h, 0)
    return h, b[off:off + size]
h, k = blk(1)
print(k[:0x40].hex(' '))
a1 = h[0x19]; a2 = struct.unpack_from('<h', h, 0x1a)[0]; nrec = h[8]
print('a1', a1, 'a2', a2, 'nrec', nrec)
for r in range(nrec):
    rec = k[a2 + 101 * r:a2 + 101 * (r + 1)]
    print('%2d' % r, rec[:2].hex(), rec[2:0x16].split(b'\0')[0].decode('big5', 'replace'), rec[0x16:].hex(' '))
p = a2 + 101 * nrec
print('after recs at', p, k[p:p + 64].hex(' '))
