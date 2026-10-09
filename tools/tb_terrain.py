import struct
R = '/home/lizhaoliu/games/super2sp/dump/tables/raw/'
b = open(R + 'dat_06', 'rb').read()
nt, ng, no = struct.unpack_from('<3h', b, 0)
print(nt, ng, no, len(b), 6 + 136 * nt * 3 + nt * 11)
names_off = 6 + 136 * nt * 3
names = [b[names_off + 11 * i:names_off + 11 * (i + 1)].split(b'\0')[0].decode('big5', 'replace') for i in range(nt)]
print(list(enumerate(names)))
k8 = open('/home/lizhaoliu/games/super2sp/extracted/app/KINDSP.BIN', 'rb').read()
def cname(c):
    o = 1355 + 37 * c; r = k8[o:o + 11]; return r[:r.index(b'\0')].decode('big5', 'replace')
for c in [0, 3, 4, 11, 13, 69, 105, 77]:
    row = b[6 + c * nt * 3:6 + (c + 1) * nt * 3]
    print(c + 1, cname(c), [tuple(struct.unpack('bbb', row[3 * t:3 * t + 3])) for t in range(nt)])
