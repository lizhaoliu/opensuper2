"""Recover the jump tables Ghidra failed on. Output lines: func jmp_addr target1,target2,..."""
import re, struct, sys
import capstone, pefile
pe = pefile.PE('/home/lizhaoliu/games/super2sp/re/WSUPERSP_unpacked.exe', fast_load=True)
img = pe.get_memory_mapped_image(); base = 0x400000
md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32); md.detail = True
sites = []
fn = None
for line in open(sys.argv[1] if len(sys.argv) > 1 else '/home/lizhaoliu/games/super2sp/re/decomp.c', encoding='utf-8', errors='replace'):
    m = re.match(r'// ==== (\S+) @ ([0-9a-f]+)', line)
    if m: fn = int(m.group(2), 16)
    m = re.search(r'Could not recover jumptable at 0x([0-9a-f]+)', line)
    if m: sites.append((fn, int(m.group(1), 16)))
def rd(a, n): return img[a - base:a - base + n]
for fn, j in sorted(set(sites)):
    insns = list(md.disasm(rd(fn, j - fn + 8), fn))
    k = next(i for i, x in enumerate(insns) if x.address == j)
    jmp = insns[k]
    m = re.search(r'\[(\w+)\*4 \+ (0x[0-9a-f]+)\]', jmp.op_str)
    if not m:
        print('#', hex(fn), hex(j), 'unhandled', jmp.op_str); continue
    tbl = int(m.group(2), 16)
    idx_tbl = None; bound = None
    for x in reversed(insns[max(0, k - 14):k]):
        mm = re.search(r'byte ptr \[(\w+) \+ (0x[0-9a-f]+)\]', x.op_str)
        if x.mnemonic == 'mov' and mm and idx_tbl is None: idx_tbl = int(mm.group(2), 16)
        if x.mnemonic == 'cmp' and bound is None:
            mb = re.search(r', (0x[0-9a-f]+|\d+)$', x.op_str)
            if mb: bound = int(mb.group(1), 0)
    if bound is None:
        print('#', hex(fn), hex(j), 'no bound'); continue
    if idx_tbl is not None:
        idx = rd(idx_tbl, bound + 1)
        nslots = max(idx) + 1
    else:
        idx = list(range(bound + 1)); nslots = bound + 1
    targets = [struct.unpack_from('<I', rd(tbl, 4 * nslots), 4 * s)[0] for s in range(nslots)]
    ok = all(fn <= t < fn + 0x10000 for t in targets)
    print('%08x %08x %s%s' % (fn, j, ','.join('%08x' % t for t in targets), '' if ok else ' #suspect'))
