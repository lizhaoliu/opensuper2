import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
b = entry('bin', 1)
def big5(x): return x.decode('big5', 'replace')
def stage(i):
    h = b[2 + 30 * (i - 1): 2 + 30 * i]
    off, size = struct.unpack_from('<II', h, 0)
    blk = b[off:off + size]
    nunits = h[8]; n4 = h[0x19]; uoff = struct.unpack_from('<I', h, 0x1a)[0]
    return h, blk, nunits, n4, uoff
i = int(sys.argv[1])
h, blk, nunits, n4, uoff = stage(i)
print('hdr', h.hex(' '))
print('pre', blk[:uoff].hex(' '))
p = uoff
for u in range(nunits):
    r = blk[p + u * 0x65: p + (u + 1) * 0x65]
    print('unit', u, r.hex(' '))
p += nunits * 0x65
h7 = struct.unpack_from('<7H', blk, p); p += 14
print('h7', h7)
name = blk[p:p + h7[0]]; p += h7[0]; print('name', repr(big5(name)))
s1 = blk[p:p + h7[1]]; p += h7[1]; print('s1', repr(big5(s1)))
s2 = blk[p:p + h7[2]]; p += h7[2]; print('s2', repr(big5(s2)))
s45 = blk[p:p + 2 * (h7[4] + h7[5])]; p += 2 * (h7[4] + h7[5]); print('s45', struct.unpack_from('<%dh' % (h7[4] + h7[5]), s45))
s3 = blk[p:p + h7[3]]; p += h7[3]; print('s3', repr(big5(s3)))
s6 = blk[p:p + 2 * h7[6]]; p += 2 * h7[6]; print('s6', struct.unpack_from('<%dh' % h7[6], s6))
scr = blk[p:]
print('script len', len(scr), scr[:4].hex(' '))
ne = struct.unpack_from('<h', scr, 0)[0]
for e in range(ne):
    r = scr[4 + 20 * e: 24 + 20 * e]
    print('ev', e, r.hex(' '))
print('==== dialogues')
for e in range(ne):
    r = scr[4 + 20 * e: 24 + 20 * e]
    typ = r[0]; dlo = struct.unpack_from('<I', r, 0xc)[0]; tlen = struct.unpack_from('<H', r, 0x10)[0]; nl = struct.unpack_from('<h', r, 0x12)[0]
    lines = scr[dlo:dlo + 0x17 * nl]; pool = scr[dlo + 0x17 * nl: dlo + 0x17 * nl + tlen]
    print('-- ev', e, 'type', typ, 'nl', nl, 'tlen', tlen)
    for k in range(nl):
        L = lines[k * 0x17:(k + 1) * 0x17]
        cid, s1, s2 = struct.unpack_from('<hhh', L, 0)
        name = L[6:0x13].split(b'\0')[0].decode('big5', 'replace')
        toff, x = struct.unpack_from('<hh', L, 0x13)
        txt = pool[toff:].split(b'\0')[0]
        print('  ', cid, s1, s2, name, toff, x, repr(txt.decode('big5', 'replace')))
