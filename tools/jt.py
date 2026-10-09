"""Dump a jump table: jt.py <targets_hex> <bytes_index_hex> <count>  (byte-indexed dword table)"""
import sys, struct, pefile
pe = pefile.PE('/home/lizhaoliu/games/super2sp/re/WSUPERSP_unpacked.exe', fast_load=True)
img = pe.get_memory_mapped_image(); base = 0x400000
t = int(sys.argv[1], 16); bi = int(sys.argv[2], 16); n = int(sys.argv[3], 0)
idx = img[bi - base: bi - base + n]
for k in range(n):
    tgt = struct.unpack_from('<I', img, t - base + 4 * idx[k])[0]
    print('case %#x -> slot %d -> %08x' % (k, idx[k], tgt))
