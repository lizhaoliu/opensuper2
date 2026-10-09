"""Replace functions in decomp.c with re-decompiled versions: merge_decomp.py base.c patch.c out.c"""
import re, sys
def split(path):
    out, cur, name = {}, [], None
    order = []
    for line in open(path, encoding='utf-8', errors='replace'):
        m = re.match(r'// ==== \S+ @ ([0-9a-f]+)', line)
        if m:
            if name: out[name] = ''.join(cur)
            name = m.group(1); cur = []; order.append(name)
        cur.append(line)
    if name: out[name] = ''.join(cur)
    return out, order
base, order = split(sys.argv[1]); patch, _ = split(sys.argv[2])
with open(sys.argv[3], 'w') as f:
    for a in order: f.write(patch.get(a, base[a]))
print('replaced', len(set(patch) & set(base)), 'of', len(base))
