import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
pal, tables = load_index()
banks = {}
for k in (3,4,5):
    e = entry('prg',k); n = struct.unpack_from('<H', e, 0)[0]
    t = [struct.unpack_from('<II', e, 2+8*i) for i in range(n)]
    banks[k] = (e, t)
def tile(k, id):
    e, t = banks[k]; o, s = t[id-1]
    w,h = struct.unpack_from('<hh', e, o)
    return w+1, h+1, e[o+4:o+s]
attrs = entry('prg',7)
m6 = entry('prg',6)
def load_map(mi):
    n = struct.unpack_from('<H', m6, 0)[0]
    o, s = struct.unpack_from('<II', m6, 2+8*mi)
    b = m6[o:o+s]
    h = struct.unpack_from('<14h', b, 0)
    W, H, ncell = h[0], h[1], h[2]
    nobj, nrec = h[6], h[7]
    p = 0x1c
    cells = [struct.unpack_from('<Bhh', b, p+5*i) for i in range(ncell)]; p += 5*ncell
    objs = [struct.unpack_from('<5h', b, p+10*i) for i in range(nobj)]; p += 10*nobj
    recs = [struct.unpack_from('<3h', b, p+6*i) for i in range(nrec)]; p += 6*nrec
    return h, cells, objs, recs, p, len(b)
def blit(can, CW, CH, x, y, w, h, pix):
    for yy in range(h):
        dy = y+yy
        if dy < 0 or dy >= CH: continue
        row = pix[yy*w:(yy+1)*w]
        for xx in range(w):
            c = row[xx]
            if c and 0 <= x+xx < CW: can[dy*CW+x+xx] = c
def render(mi, path):
    h, cells, objs, recs, p, L = load_map(mi)
    W, H = h[0], h[1]
    print('map', mi, h, 'objs', len(objs), objs[:3], 'recs', recs[:3], 'p', p, 'len', L)
    CW, CH = W*64, H*32; top = 0
    can = bytearray(CW*CH)
    p8 = entry('prg', 8)
    def big(id, x, y, opaque):
        if id <= 0: return
        o, s = struct.unpack_from('<II', p8, 2+8*(id-1))
        fl = p8[o]; w, hh = struct.unpack_from('<hh', p8, o+1); w += 1; hh += 1
        pix = rle_decode(p8, o+1)[2] if fl == 0 else p8[o+5:o+5+w*hh]
        for yy in range(hh):
            dy = y+yy
            if not 0 <= dy < CH: continue
            for xx in range(w):
                dx = x+xx
                if 0 <= dx < CW and (opaque or pix[yy*w+xx]): can[dy*CW+dx] = pix[yy*w+xx]
    big(h[8], h[9], h[10], True)
    def height(x, y):
        if y < 1 or y >= 2*H: return 0
        if y % 2 == 0 and (x == 0 or x == W-1): return 0
        return cells[y*W+x][0]
    # objects with computed screen pos
    ob = []
    for (ox, oy, ow, oh, oid) in objs:
        hh = height(ox + (ow >> 7), oy - 2 + (oh >> 4))
        sx = ox*64 + (32 if oy & 1 else 0)
        sy = oy*16 - ((hh-1)*16 if hh > 1 else 0)
        ob.append((oy*16+oh, sx, sy, oid))
    for r in range(2*H):
        for (key, sx, sy, oid) in ob:
            if key == r*16:
                w,hh,pix = tile(5, oid); blit(can, CW, CH, sx, sy+top, w, hh, pix)
        ncol = W if r % 2 == 0 else W-1
        for c in range(ncol):
            x = c*64 + (32 if r & 1 else 0); y = r*16
            ht, g, ov = cells[r*W+c]
            if ht == 0: continue
            w,hh,pix = tile(3, g)
            if g < 6: blit(can, CW, CH, x, y+top, w, hh, pix)
            else:
                for k in range(ht): blit(can, CW, CH, x, y - k*16 + top, w, hh, pix)
                if ov > 0:
                    w2,h2,p2 = tile(4, ov); blit(can, CW, CH, x, y - (ht-1)*16 - 64 + top, w2, h2, p2)
    for (key, sx, sy, oid) in ob:
        if key > (2*H-1)*16:
            w,hh,pix = tile(5, oid); blit(can, CW, CH, sx, sy+top, w, hh, pix)
    big(h[11], h[12], h[13], False)
    write_png(path, CW, CH, bytes(can), pal, transparent=False)
for mi in [int(x) for x in sys.argv[1:]]:
    render(mi, '/home/lizhaoliu/games/super2sp/dump/gfx/map_%03d.png' % mi)
