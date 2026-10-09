import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
pal, tables = load_index()
cands = {'master': pal}
for i in list(range(11,21)) + list(range(26,41)):
    cands['pcx%d'%i] = entry('dat2', i)[-768:]
def score(w, h, pix, p):
    lum = [0.3*p[c*3]+0.59*p[c*3+1]+0.11*p[c*3+2] for c in range(256)]
    s = 0; n = 0
    for y in range(0, h-1, 2):
        for x in range(0, w-1, 2):
            a = pix[y*w+x]; b = pix[y*w+x+1]; c = pix[(y+1)*w+x]
            if a == 0 or b == 0: continue
            s += abs(lum[a]-lum[b]) + abs(lum[a]-lum[c]); n += 1
    return s / max(n, 1)
def test(name, w, h, pix):
    r = sorted((score(w, h, pix, p), k) for k, p in cands.items())
    print(name, [(k, round(v,1)) for v, k in r[:4]])
e = entry('dat2', 10)
for j in (0, 6, 12, 13):
    o, s = struct.unpack_from('<II', e, 2+8*j)
    r = rle_decode(e, o); test('dat10_%d'%j, r[0], r[1], r[2])
e = entry('dat2', 7)
for j in (0, 6, 10):
    o, s = struct.unpack_from('<II', e, 2+8*j)
    w, h, pix, _ = flagged(e, o); test('dat7_%d'%j, w, h, pix)
