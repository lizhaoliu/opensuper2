import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct, collections
from s2 import *
g = entry('bin', 17)
n = struct.unpack_from('<H', g, 0)[0]
ents = [struct.unpack_from('<II', g, 2 + 8 * i) for i in range(n)]
fmts = collections.Counter(); bad = []
tot = 0
for i, (o, s) in enumerate(ents):
    w = g[o:o + s]
    riff = struct.unpack_from('<I', w, 4)[0]
    p = 12; fmt = None; data = None
    while p + 8 <= len(w):
        cid = w[p:p + 4]; cl = struct.unpack_from('<I', w, p + 4)[0]
        if cid == b'fmt ': fmt = struct.unpack_from('<HHIIHH', w, p + 8)
        if cid == b'data': data = (p + 8, cl)
        p += 8 + cl + (cl & 1)
    if riff + 8 != s: bad.append((i + 1, 'riff', riff + 8, s))
    if not fmt or not data: bad.append((i + 1, 'nochunk')); continue
    tag, ch, rate, bps, align, bits = fmt
    fmts[(tag, ch, rate, bits)] += 1
    dur = data[1] / bps
    tot += dur
    if data[0] + data[1] > s: bad.append((i + 1, 'data overrun', data, s))
print(fmts.most_common()); print('bad', bad[:10]); print('total seconds %.1f' % tot)
# per id durations, compact
durs = []
for i, (o, s) in enumerate(ents):
    w = g[o:o + s]; bps = struct.unpack_from('<I', w, 28)[0]
    durs.append('%d:%.1f' % (i + 1, (s - 44) / bps))
print(' '.join(durs))
# DAT entries heads
pal, t = load_index()
dat = open(D + FILES['dat2'], 'rb')
for i, (o, s) in enumerate(t['dat2']):
    dat.seek(o); h = dat.read(16)
    print('dat2', i + 1, o, s, h.hex(' '))
