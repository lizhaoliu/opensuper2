"""Montage arbitrary indexed PNG dumps scaled to fit cells (reads our own PNG output)."""
import os, sys, struct, zlib
def read_png(p):
    b = open(p,'rb').read(); pos = 8; idat = b''; pal = None
    while pos < len(b):
        ln, = struct.unpack('>I', b[pos:pos+4]); t = b[pos+4:pos+8]; d = b[pos+8:pos+8+ln]; pos += 12+ln
        if t == b'IHDR': w, h = struct.unpack('>II', d[:8])
        elif t == b'PLTE': pal = d
        elif t == b'IDAT': idat += d
    raw = zlib.decompress(idat); pix = bytearray(); prev = bytearray(w)
    for y in range(h):
        ft = raw[y*(w+1)]; line = bytearray(raw[y*(w+1)+1:(y+1)*(w+1)])
        for x in range(w):
            a = line[x-1] if x else 0; b = prev[x]; c = prev[x-1] if x else 0
            if ft == 1: line[x] = (line[x] + a) & 255
            elif ft == 2: line[x] = (line[x] + b) & 255
            elif ft == 3: line[x] = (line[x] + (a + b) // 2) & 255
            elif ft == 4:
                pp = a + b - c; pa, pb, pc = abs(pp-a), abs(pp-b), abs(pp-c)
                line[x] = (line[x] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        pix += line; prev = line
    return w, h, bytes(pix), pal
if __name__ == '__main__':
    out, cw, ch, cols = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4])
    files = sys.argv[5:]
    rows = (len(files)+cols-1)//cols; W, H = cw*cols, ch*rows
    rgb = bytearray(b'\x30\x50\x30' * (W*H))
    for j, f in enumerate(files):
        w, h, pix, pal = read_png(f); s = max(w/cw, h/ch, 1)
        ox, oy = (j%cols)*cw, (j//cols)*ch
        for y in range(min(ch, int(h/s))):
            for x in range(min(cw, int(w/s))):
                c = pix[int(y*s)*w+int(x*s)]; q = ((oy+y)*W+ox+x)*3
                rgb[q:q+3] = pal[c*3:c*3+3]
    raw = b''.join(b'\0' + bytes(rgb[y*W*3:(y+1)*W*3]) for y in range(H))
    def chunk(t, d): return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    open(out, 'wb').write(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', W, H, 8, 2, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(raw, 6)) + chunk(b'IEND', b''))

