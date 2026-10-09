import struct, sys
R = '/home/lizhaoliu/games/super2sp/dump/tables/raw/'
b = open(R + 'bin_01', 'rb').read()
def stage(st):
    h = b[2 + 30 * (st - 1):2 + 30 * st]
    off, size = struct.unpack_from('<ii', h, 0)
    k = b[off:off + size]
    a1 = h[0x19]; a2 = struct.unpack_from('<h', h, 0x1a)[0]; nrec = h[8]
    print('stage', st, 'hdr', h[8:].hex(' '), 'a1', a1, 'a2', a2, 'nrec', nrec)
    t1 = [struct.unpack_from('<hh', k, 4 * i) for i in range(a1)]
    print(' tab1', t1, ' tab2 raw', k[4 * a1:a2].hex(' '))
    p = a2 + 101 * nrec
    L = struct.unpack_from('<7H', k, p)
    print(' L', L)
    q = p + 14
    def s(n):
        nonlocal q
        d = k[q:q + n]; q += n; return d
    name = s(L[0]); sec2 = s(L[1]); sec3 = s(L[2]); arr = s(L[4] * 2 + L[5] * 2); sec4 = s(L[3]); arr6 = s(L[6] * 2)
    dec = lambda d: d.replace(b'\0', b'|').decode('big5', 'replace')
    print(' name', dec(name)); print(' sec2', dec(sec2)); print(' sec3', dec(sec3), sec3.hex(' '))
    print(' arr45', struct.unpack('<%dh' % (L[4] + L[5]), arr)); print(' sec4', sec4.hex(' ')); print(' arr6', struct.unpack('<%dh' % L[6], arr6))
    print(' script starts at', q, 'of', len(k), k[q:q + 48].hex(' '))
for st in map(int, sys.argv[1:]): stage(st)
