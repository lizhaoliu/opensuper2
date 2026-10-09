import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
pal, tables = load_index()
def load_png_like(k):
    e = entry('cel',2); o,s = struct.unpack_from('<II', e, 2+8*(k-0x8f))
    return rle_decode(e, o)
def montage(ids, path, scale=2, cols=4, cellw=320, cellh=240):
    W, H = cellw*cols, cellh*((len(ids)+cols-1)//cols)
    can = bytearray(W*H)
    for j,k in enumerate(ids):
        w,h,pix,_ = load_png_like(k)
        ox, oy = (j%cols)*cellw, (j//cols)*cellh
        for y in range(0, min(h, cellh*scale), scale):
            row = pix[y*w:(y+1)*w:scale][:cellw]
            can[(oy+y//scale)*W+ox:(oy+y//scale)*W+ox+len(row)] = row
    p2 = bytearray(pal); p2[0:3]=b'\x30\x50\x30'
    write_png(path, W, H, bytes(can), bytes(p2), transparent=False)
ids = [int(x) for x in sys.argv[2:]]
montage(ids, sys.argv[1])
