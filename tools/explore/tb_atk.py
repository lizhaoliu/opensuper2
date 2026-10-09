import struct
k8 = open('/home/lizhaoliu/games/super2sp/extracted/app/KINDSP.BIN', 'rb').read()
def cname(c):
    o = 1355 + 37 * c; r = k8[o:o + 11]; return r[:r.index(b'\0')].decode('big5', 'replace')
base = 7179 + 136 * 4
for c in [0, 1, 3, 4, 6, 8, 80, 60, 30, 121]:
    st, cn = struct.unpack_from('<HH', k8, 7179 + 4 * c)
    print(c + 1, cname(c))
    for j in range(st, st + cn):
        r = k8[base + 23 * j:base + 23 * (j + 1)]
        print('   %3d' % j, ' '.join('%4d' % x for x in struct.unpack('23b', r)))
