import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
for which in (5, 6):
    b = entry('bin', which)
    n = struct.unpack_from('<h', b, 0)[0]
    base = 2 + n * 8
    with open('/home/lizhaoliu/games/super2sp/dump/flow/msgs_bin%d.txt' % which, 'w') as f:
        for i in range(n):
            off, sz = struct.unpack_from('<II', b, 2 + 8 * i)
            s = b[base + off: base + off + sz].split(b'\0')[0]
            f.write('%#05x %s\n' % (i, s.decode('big5', 'replace').replace('\n', '\\n')))
    print(which, n)
