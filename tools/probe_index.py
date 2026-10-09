import struct, os
D='/home/lizhaoliu/games/super2sp/extracted/app/'
b=open(D+'SUPERSP.PAL','rb').read()
p=list(struct.unpack_from('<%dI'%((len(b)-0x300)//4), b, 0x300))
print(len(b), len(p))
subs=[0x38,0xb,8,10,0x14,0x38,0xb,8,10,0x14]
cnt=[(p[i]-subs[i])&0xffffffff for i in range(10)]
print('counts', cnt)
names=['cel','bin','dat1','prg','dat2']
pos=10
tables={}
for k,(name,base) in enumerate(zip(names,[0x38,0xb,8,10,0x14])):
    n=cnt[k]
    t=[]
    for i in range(n):
        o=(p[pos+2*i]-(base+i))&0xffffffff; s=(p[pos+2*i+1]-(base+i))&0xffffffff
        t.append((o,s))
    pos+=2*n
    tables[name]=t
    print(name,n,t[:4],'...',t[-2:])
print('used u32', pos, 'of', len(p))
files={'cel':'SUPERSP.CEL','bin':'SUPERSP.BIN','prg':'SUPERSP.PRG','dat1':'SUPERSP/SUPERSP.DAT','dat2':'SUPERSP/SUPERSP.DAT'}
for k,t in tables.items():
    sz=os.path.getsize(D+files[k])
    print(k, 'last end', hex(t[-1][0]+t[-1][1]) if t else None, 'filesize', hex(sz))
