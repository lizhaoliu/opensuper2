"""Shared helpers for poking at SuperSp data (exploration only)."""
import struct, zlib
D = '/home/lizhaoliu/games/super2sp/extracted/app/'
FILES = {'cel': 'SUPERSP.CEL', 'bin': 'SUPERSP.BIN', 'prg': 'SUPERSP.PRG', 'dat1': 'SUPERSP/SUPERSP.DAT', 'dat2': 'SUPERSP/SUPERSP.DAT'}

def load_index():
    b = open(D + 'SUPERSP.PAL', 'rb').read()
    pal = bytes(min(255, (c << 2) | (c >> 4)) for c in b[:0x300])
    p = struct.unpack_from('<%dI' % ((len(b) - 0x300) // 4), b, 0x300)
    bases = [0x38, 0xb, 8, 10, 0x14]
    cnt = [(p[i] - bases[i]) & 0xffffffff for i in range(5)]
    pos, tables = 10, {}
    for k, name in enumerate(['cel', 'bin', 'dat1', 'prg', 'dat2']):
        t = []
        for i in range(cnt[k]):
            t.append(((p[pos + 2 * i] - (bases[k] + i)) & 0xffffffff, (p[pos + 2 * i + 1] - (bases[k] + i)) & 0xffffffff))
        pos += 2 * cnt[k]
        tables[name] = t
    return pal, tables

_cache = {}
def entry(kind, idx):  # idx is 1-based like the game
    if kind not in _cache:
        _cache[kind] = open(D + FILES[kind], 'rb').read()
    _, tables = load_index()
    o, s = tables[kind][idx - 1]
    return _cache[kind][o:o + s]

def big5(b): return b.decode("big5", "replace")

def rle_decode(b, off=0):
    w, h = struct.unpack_from('<hh', b, off); w += 1; h += 1
    out = bytearray(); p = off + 4; n = w * h
    while len(out) < n:
        c = b[p]; p += 1
        if c == 0: out.append(0)
        elif c & 0x80:
            k = c & 0x7f; out += b[p:p + k]; p += k
        else:
            out += bytes([b[p]]) * c; p += 1
    return w, h, bytes(out[:n]), p

def write_png(path, w, h, pix, pal, transparent=True):
    raw = b''.join(b'\0' + pix[y * w:(y + 1) * w] for y in range(h))
    def chunk(t, d): return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 3, 0, 0, 0)) + chunk(b'PLTE', pal)
    if transparent: png += chunk(b'tRNS', b'\0')
    png += chunk(b'IDAT', zlib.compress(raw, 9)) + chunk(b'IEND', b'')
    open(path, 'wb').write(png)

def sheet(frames, path, pal, cols=12, bg=b'\x30\x50\x30'):
    """frames: list of (w, h, pix, ...). Writes a contact sheet."""
    W = max(f[0] for f in frames); H = max(f[1] for f in frames)
    cols = min(cols, len(frames)); rows = (len(frames) + cols - 1) // cols
    sh = bytearray(W * cols * H * rows)
    for i, f in enumerate(frames):
        w, h, pix = f[0], f[1], f[2]
        ox, oy = (i % cols) * W, (i // cols) * H
        for y in range(h):
            sh[(oy + y) * W * cols + ox:(oy + y) * W * cols + ox + w] = pix[y * w:(y + 1) * w]
    p2 = bytearray(pal); p2[0:3] = bg
    write_png(path, W * cols, H * rows, bytes(sh), bytes(p2), transparent=False)

def flagged(b, o):
    """5-byte-header image: u8 flag (0 RLE / 1 raw), i16 w-1, i16 h-1."""
    fl = b[o]; w, h = struct.unpack_from('<hh', b, o + 1); w += 1; h += 1
    if fl == 0:
        r = rle_decode(b, o + 1); return r[0], r[1], r[2], r[3] - o
    return w, h, bytes(b[o + 5:o + 5 + w * h]), 5 + w * h
