"""For a recovered switch, print target -> case values. casemap.py <jmp_hex> [bias]"""
import re, sys, struct, collections
import capstone, pefile
pe = pefile.PE('/home/lizhaoliu/games/super2sp/re/WSUPERSP_unpacked.exe', fast_load=True)
img = pe.get_memory_mapped_image(); base = 0x400000
md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
j = int(sys.argv[1], 16); bias = int(sys.argv[2], 0) if len(sys.argv) > 2 else 0
fn = None
for l in open('/home/lizhaoliu/games/super2sp/dump/flow/switches.txt'):
    p = l.split()
    if len(p) >= 3 and int(p[1], 16) == j: fn = int(p[0], 16)
insns = list(md.disasm(img[fn - base:j - base + 8], fn))
k = next(i for i, x in enumerate(insns) if x.address == j)
tbl = int(re.search(r'\+ (0x[0-9a-f]+)\]', insns[k].op_str).group(1), 16)
idx_tbl = bound = None
for x in reversed(insns[max(0, k - 14):k]):
    mm = re.search(r'byte ptr \[(\w+) \+ (0x[0-9a-f]+)\]', x.op_str)
    if x.mnemonic == 'mov' and mm and idx_tbl is None: idx_tbl = int(mm.group(2), 16)
    if x.mnemonic == 'cmp' and bound is None:
        mb = re.search(r', (0x[0-9a-f]+|\d+)$', x.op_str)
        if mb: bound = int(mb.group(1), 0)
idx = img[idx_tbl - base: idx_tbl - base + bound + 1] if idx_tbl else bytes(range(bound + 1))
m = collections.defaultdict(list)
for v in range(bound + 1):
    m[struct.unpack_from('<I', img, tbl - base + 4 * idx[v])[0]].append(v + bias)
for t, vs in sorted(m.items(), key=lambda kv: kv[1][0]):
    print('%08x: %s' % (t, ' '.join(hex(v) for v in vs) if len(vs) < 20 else '%d values (default)' % len(vs)))
