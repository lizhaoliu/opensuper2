import struct, sys
D='/home/lizhaoliu/games/super2sp/extracted/app/'
def hdr(path):
    b=open(path,'rb').read()
    size,magic=struct.unpack_from('<I4s',b,0)
    name=b[8:18]; ver=b[18:22]; cnt=struct.unpack_from('<I',b,24)[0]
    return b,size,name,ver,cnt
b,size,name,ver,cnt=hdr(D+'SUPERSP.CEL')
print(name,ver,cnt,len(b)-28==size)
p=28
n=struct.unpack_from('<H',b,p)[0]
ents=[struct.unpack_from('<II',b,p+2+8*i) for i in range(n)]
print(n, ents[:5], ents[-3:], hex(p+ents[-1][0]+ents[-1][1]), hex(len(b)))
# entry 0 first bytes
o,s=ents[0]
print(b[p+o:p+o+64].hex(' '))
o,s=ents[1]
print(b[p+o:p+o+64].hex(' '))
