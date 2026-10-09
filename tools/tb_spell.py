import struct
R = '/home/lizhaoliu/games/super2sp/dump/tables/raw/'
b = open(R + 'bin_02', 'rb').read()
n1, n7, n8 = b[1], b[7], b[8]
print('hdr', b[:9].hex(' '), 'schools', n1, 'spells', n7, 'help', n8)
sch = [(b[9 + 2 * i], b[10 + 2 * i]) for i in range(n1)]
print('school (first,count)', sch)
sp = 9 + n1 * 2
st = sp + n7 * 37
offs = struct.unpack_from('<%dH' % (n1 + n8), b, st)
sbase = st + (n1 + n8) * 2
strs = [b[sbase + o:b.index(b'\0', sbase + o)].decode('big5', 'replace') for o in offs]
print('school names', strs[:n1]); print('targets?', strs[n1:])
for i in range(n7):
    r = b[sp + 37 * i:sp + 37 * (i + 1)]
    nm = r[1:13].split(b'\0')[0].decode('big5', 'replace')
    print('%3d t%-2d %-8s' % (i + 1, r[0], nm), r[13:].hex(' '))
