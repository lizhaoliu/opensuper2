import struct
k8 = open('/home/lizhaoliu/games/super2sp/extracted/app/KINDSP.BIN', 'rb').read()
base = 7179 + 136 * 4
seen = {}
for j in range(378):
    r = k8[base + 23 * j:base + 23 * (j + 1)]
    nm = r[1:11].split(b'\0')[0].decode('big5', 'replace')
    key = (r[0], nm, r[11:].hex())
    seen.setdefault(key, []).append(j)
for (t, nm, rest), js in sorted(seen.items()):
    print('%3d %-8s %s n=%d' % (t, nm, ' '.join('%4d' % x for x in struct.unpack('12b', bytes.fromhex(rest))), len(js)))
