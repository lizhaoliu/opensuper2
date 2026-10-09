import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
b = open(D + 'KINDSP.BIN', 'rb').read()
def big5z(b, o):
    e = b.index(b'\0', o); return b[o:e].decode('big5', 'replace'), e + 1
h = {}
h['off0'] = struct.unpack_from('<i', b, 0)[0]
h['n4'] = struct.unpack_from('<h', b, 4)[0]
h['str1_off'] = struct.unpack_from('<i', b, 6)[0]; h['str1_n'] = struct.unpack_from('<h', b, 10)[0]
h['str2_off'] = struct.unpack_from('<i', b, 12)[0]; h['str2_n'] = struct.unpack_from('<h', b, 16)[0]
h['off12'] = struct.unpack_from('<i', b, 0x12)[0]; h['n16'] = struct.unpack_from('<h', b, 0x16)[0]
h['off18'] = struct.unpack_from('<i', b, 0x18)[0]; h['n1c'] = struct.unpack_from('<h', b, 0x1c)[0]
h['off1e'] = struct.unpack_from('<i', b, 0x1e)[0]; h['n22'] = struct.unpack_from('<h', b, 0x22)[0]
print(h, len(b))
o = h['str1_off']; s1 = []
for i in range(h['str1_n']):
    s, o = big5z(b, o); s1.append(s)
print('str1', s1)
o = h['str2_off']; s2 = []
for i in range(h['str2_n']):
    s, o = big5z(b, o); s2.append(s)
print('str2', s2)
print('region off0..str1', h['off0'], h['str1_off'], (h['str1_off'] - h['off0']), (h['str1_off'] - h['off0']) / h['n4'])
print(b[h['off0']:h['off0'] + 64].hex(' '))
print('off12 region', h['off12'], h['off18'], (h['off18'] - h['off12']), (h['off18'] - h['off12']) / h['n16'])
print(b[h['off12']:h['off12'] + 96].hex(' '))
n = h['n1c']; m = b[h['off18']:h['off18'] + n * n]
for r in range(n): print(' '.join('%3d' % x for x in m[r * n:(r + 1) * n]))
print('after matrix', h['off18'] + n * n, 'off1e', h['off1e'])
print(b[h['off18'] + n * n:h['off1e']].hex(' '))
print(b[h['off1e']:h['off1e'] + 64].hex(' '), len(b) - h['off1e'])
print('====types')
for i in range(24):
    o = 6963 + 9 * i; print(i, b[o:o + 9].split(b'\0')[0].decode('big5', 'replace'), end=' | ')
print()
print('====classes')
for i in range(136):
    o = h['off12'] + 37 * i; r = b[o:o + 37]
    name = r[:r.index(b'\0')].decode('big5', 'replace') if b'\0' in r[:10] else r[:10]
    a = b[h['off0'] + 8 * i:h['off0'] + 8 * i + 8]
    st, cn = struct.unpack_from('<HH', b, h['off1e'] + 4 * i)
    print('%3d %-8s' % (i, name), r[4:].hex(' '), '| tree', list(a), '| list', st, cn)
