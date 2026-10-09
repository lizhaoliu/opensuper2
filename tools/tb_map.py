import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
p6 = entry('prg', 6); p7 = entry('prg', 7)
open('/home/lizhaoliu/games/super2sp/dump/tables/raw/prg_06', 'wb').write(p6)
open('/home/lizhaoliu/games/super2sp/dump/tables/raw/prg_07', 'wb').write(p7)
for i in (3, 4, 5):
    e = entry('prg', i)
    print('prg', i, len(e), 'count', struct.unpack_from('<h', e, 0)[0], e[:24].hex(' '))
print('prg7', len(p7), struct.unpack_from('<3h', p7, 0))
n = struct.unpack_from('<h', p6, 0)[0]
print('maps', n, len(p6))
for i in range(n):
    o, s = struct.unpack_from('<II', p6, 2 + 8 * i)
    h = struct.unpack_from('<14h', p6, o)
    print(i + 1, o, s, h)
