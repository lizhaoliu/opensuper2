import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
pal, tables = load_index()
out = '/home/lizhaoliu/games/super2sp/dump/gfx/ui/'
os.makedirs(out, exist_ok=True)
imgs = {}
e = entry('dat2',5); n = struct.unpack_from('<H', e, 0)[0]
for i in range(n):
    o,s = struct.unpack_from('<II', e, 2+8*i)
    w,h = struct.unpack_from('<hh', e, o); w+=1; h+=1
    imgs[i+1] = (w,h,e[o+4:o+4+w*h])
e = entry('cel',2); n = struct.unpack_from('<H', e, 0)[0]
for i in range(n):
    o,s = struct.unpack_from('<II', e, 2+8*i)
    imgs[0x8e+1+i] = rle_decode(e, o)
for k,(w,h,pix,*_) in imgs.items():
    write_png(out+'img%03d.png'%k, w, h, pix, pal)
# sheets of small icons
small = [k for k,v in imgs.items() if v[0]<=64 and v[1]<=64]
for j in range(0, len(small), 120):
    ks = small[j:j+120]
    sheet([imgs[k] for k in ks], out+'../ui_small_%03d.png'%j, pal, cols=20)
    print(j, ks[0], ks[-1])
