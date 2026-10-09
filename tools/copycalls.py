import sys, json, capstone, pefile, re
pe = pefile.PE('/home/lizhaoliu/games/super2sp/re/WSUPERSP_unpacked.exe', fast_load=True)
img = pe.get_memory_mapped_image(); base = 0x400000
md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
sizes = {int(json.loads(l)['addr'], 16): json.loads(l)['size'] for l in open('/home/lizhaoliu/games/super2sp/re/graph.jsonl')}
for a in sys.argv[1:]:
    f = int(a, 16); n = sizes[f]
    ins = list(md.disasm(img[f - base:f - base + n + 16], f))
    out = []
    for i, x in enumerate(ins):
        if x.mnemonic == 'call' and x.op_str == '0x46ec6b':
            pushes = [y.op_str for y in ins[max(0, i - 6):i] if y.mnemonic == 'push']
            a1, a2 = pushes[-1], pushes[-2]
            out.append('%x: copy %s -> %s' % (x.address, a1, a2))
    print(a, '|', '; '.join(out))
