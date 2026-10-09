import struct
b = open('/home/lizhaoliu/games/super2sp/extracted/app/ITEMSP.BIN', 'rb').read()
n = struct.unpack_from('<h', b, 0)[0]
print('items', n, (len(b) - 2) / 38)
for i in range(n):
    r = b[2 + 38 * i:2 + 38 * (i + 1)]
    nm = r[:r.index(b'\0')].decode('big5', 'replace') if b'\0' in r[:14] else r[:14]
    print('%3d %-10s' % (i + 1, nm), r[:14].hex(), '|', r[14:].hex(' '))
