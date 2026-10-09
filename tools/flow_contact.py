"""Tile several screenshots (indexed PNG from s2run) at 1/4 scale into one RGB sheet."""
import sys, zlib, struct
def read_png(p):
    b = open(p, 'rb').read(); pos = 8; idat = b''; pal = None
    while pos < len(b):
        n = struct.unpack('>I', b[pos:pos+4])[0]; t = b[pos+4:pos+8]; d = b[pos+8:pos+8+n]; pos += 12 + n
        if t == b'IHDR': w, h = struct.unpack('>II', d[:8])
        elif t == b'PLTE': pal = d
        elif t == b'IDAT': idat += d
    raw = zlib.decompress(idat); px = bytearray(); s = w + 1; prev = bytearray(w)
    for y in range(h):
        f = raw[y*s]; line = bytearray(raw[y*s+1:(y+1)*s])
        for i in range(w):
            a = line[i-1] if i else 0; b = prev[i]; c = prev[i-1] if i else 0
            if f == 1: line[i] = (line[i] + a) & 255
            elif f == 2: line[i] = (line[i] + b) & 255
            elif f == 3: line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                pp = a + b - c; pa, pb, pc = abs(pp-a), abs(pp-b), abs(pp-c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        px += line; prev = line
    return w, h, px, pal
files = sys.argv[2:]; cols = 4; S = 4; cw, ch = 160, 120
rows = (len(files)+cols-1)//cols; W, H = cw*cols, ch*rows; rgb = bytearray(W*H*3)
for k, f in enumerate(files):
    w, h, px, pal = read_png(f); ox, oy = (k%cols)*cw, (k//cols)*ch
    for y in range(ch):
        for x in range(cw):
            c = px[(y*S)*w + x*S]; o = ((oy+y)*W + ox + x)*3; rgb[o:o+3] = pal[c*3:c*3+3]
raw = b''.join(b'\0' + bytes(rgb[y*W*3:(y+1)*W*3]) for y in range(H))
def chunk(t, d): return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t+d) & 0xffffffff)
open(sys.argv[1], 'wb').write(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', W, H, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b''))
