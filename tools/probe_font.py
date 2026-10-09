import os, sys; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import struct
from s2 import *
idx = entry('bin', 11); g16 = entry('bin', 12); g24 = entry('bin', 13); g16b = entry('bin', 14)
def code(hi, lo):
    if hi == 0xc6 and lo > 0xa0: return lo + 0x7f60
    if 0xa1 <= hi <= 0xa3: base, hi = 0x8400, hi - 0xa1
    elif 0xa4 <= hi <= 0xc6: base, hi = 0x8800, hi - 0xa4
    elif 199 <= hi <= 200: base, hi = 0x805f, hi - 199
    elif 0xc9 <= hi <= 0xf9: base, hi = 0x9d19, hi - 0xc9
    else: return 0
    if 0x40 <= lo <= 0x7e: lo -= 0x40
    elif 0xa1 <= lo <= 0xfe: lo -= 0x62
    else: return 0
    return hi * 0x9d + lo + base
def glyph(ch, bank, size):
    hi, lo = ch.encode('big5')
    c = code(hi, lo)
    n = struct.unpack_from('<H', idx, c * 2 - 0x10000)[0]
    bpr = size // 8
    b = bank[(n - 1) * size * bpr:(n) * size * bpr]
    return n, [''.join('#' if b[r * bpr + x // 8] & (0x80 >> (x % 8)) else '.' for x in range(size)) for r in range(size)]
for ch in '第一':
    n, rows = glyph(ch, g16, 16); print(ch, n); print('\n'.join(rows))
n, rows = glyph('劍', g24, 24); print('\n'.join(rows))
n, rows = glyph('劍', g16b, 16); print('\n'.join(rows))
print(len(idx)//2, hex(0x8000+len(idx)//2))
