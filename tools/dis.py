"""Disassemble the unpacked exe: dis.py START_HEX [END_HEX | +COUNT] [--grep PATTERN]"""
import sys, pefile, capstone
pe = pefile.PE('/home/lizhaoliu/games/super2sp/re/WSUPERSP_unpacked.exe', fast_load=True)
img = pe.get_memory_mapped_image(); base = 0x400000
a = int(sys.argv[1], 16)
end = a + 0x200
if len(sys.argv) > 2 and not sys.argv[2].startswith('--'):
    end = a + int(sys.argv[2][1:], 16) if sys.argv[2].startswith('+') else int(sys.argv[2], 16)
md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
for i in md.disasm(img[a - base:end - base], a):
    print('%08x  %-8s %s' % (i.address, i.mnemonic, i.op_str))
