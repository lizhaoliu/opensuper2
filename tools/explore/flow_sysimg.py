import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
pal, _ = load_index()
out = '/home/lizhaoliu/games/super2sp/dump/flow/sysimg/'
os.makedirs(out, exist_ok=True)
def table(b):
    n = struct.unpack_from('<H', b, 0)[0]
    return [struct.unpack_from('<II', b, 2 + 8 * i) for i in range(n)]
imgs = []
d5 = entry('dat2', 5)
for o, s in table(d5): imgs.append(d5[o:o + s])
c2 = entry('cel', 2)
t2 = table(c2)
print('dat5', len(imgs), 'cel2', len(t2))
for o, s in t2[:58]: imgs.append(c2[o:o + s])
info = []
for i, b in enumerate(imgs):
    w, h = struct.unpack_from('<hh', b, 0); w += 1; h += 1
    kind = 'raw' if len(b) == w * h + 4 else 'rle'
    try:
        if kind == 'raw':
            pix = b[4:4 + w * h]
        else:
            _, _, pix, _ = rle_decode(b)
        write_png(out + '%03d.png' % i, w, h, pix, pal)
    except Exception as ex:
        kind += '!' + str(ex)[:30]
    info.append('%3d %-4s %4dx%-4d %d' % (i, kind, w, h, len(b)))
open(out + 'index.txt', 'w').write('\n'.join(info))
print('\n'.join(info))
