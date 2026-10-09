import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
pal, tables = load_index()
def unit_frames(idxk, datk, t):
    idx = entry(*idxk); dat = entry(*datk)
    nf, start, size = struct.unpack_from('<HHI', idx, 2+8*t)
    out=[]
    for f in range(start, start+nf):
        o,s,fl = struct.unpack_from('<IHb', dat, 2+7*f)
        out.append(rle_decode(dat, o))
    return out
if __name__ == '__main__':
    which = sys.argv[1]; t = int(sys.argv[2])
    fr = unit_frames(('prg',1),('prg',2),t) if which=='prg' else unit_frames(('cel',4),('cel',5),t)
    sheet(fr, '/home/lizhaoliu/games/super2sp/dump/gfx/unit_%s_%03d.png'%(which,t), pal)
