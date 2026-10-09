"""Print NUL-terminated Big5 strings at virtual addresses of the unpacked exe."""
import sys
EXE = '/home/lizhaoliu/games/super2sp/re/WSUPERSP_unpacked.exe'
b = open(EXE, 'rb').read()
def s(va):
    o = va - 0x400000; e = b.index(b'\0', o); return b[o:e].decode('big5', 'replace')
if __name__ == '__main__':
    a = int(sys.argv[1], 16); n = int(sys.argv[2]) if len(sys.argv) > 2 else 1; step = int(sys.argv[3], 16) if len(sys.argv) > 3 else 0
    for i in range(n):
        va = a + i * step if step else a
        print(hex(va), repr(s(va)))
        if not step:
            a = a + len(s(a).encode('big5', 'replace')) + 1
