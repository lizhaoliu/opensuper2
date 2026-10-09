"""Disassemble the unpacked exe: disasm.py <hex addr> <nbytes>"""
import sys, capstone
EXE = '/home/lizhaoliu/games/super2sp/re/WSUPERSP_unpacked.exe'
b = open(EXE, 'rb').read()
a = int(sys.argv[1], 16); n = int(sys.argv[2])
md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
for i in md.disasm(b[a - 0x400000:a - 0x400000 + n], a):
    print('%x  %-8s %s' % (i.address, i.mnemonic, i.op_str))
