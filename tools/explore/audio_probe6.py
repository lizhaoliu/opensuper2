import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct, collections
from s2 import *
g = entry('bin', 18)
n = struct.unpack_from('<H', g, 0)[0]
ents = [struct.unpack_from('<II', g, 2 + 8 * i) for i in range(n)]
def chunks(b, p, end, depth=0, out=None):
    while p + 8 <= end:
        cid = b[p:p+4]; ln = struct.unpack_from('>I', b, p+4)[0]
        if cid in (b'FORM', b'CAT '):
            out.append(('  '*depth) + cid.decode() + ' ' + b[p+8:p+12].decode() + ' %d' % ln)
            chunks(b, p + 12, p + 8 + ln, depth + 1, out)
        else:
            out.append(('  '*depth) + cid.decode() + ' %d' % ln + ' ' + b[p+8:p+8+min(ln,12)].hex(' '))
        p += 8 + ln + (ln & 1)
    return out
stat = collections.Counter()
for i in (0, 5, 36):
    o, s = ents[i]
    print(i + 1, s); print('\n'.join(chunks(g, o, o + s, 1, [])))
for o, s in ents:
    for line in chunks(g, o, o + s, 0, []):
        stat[line.split()[0] if not line.startswith(' ') else line.strip().split()[0]] += 1
print(stat)
