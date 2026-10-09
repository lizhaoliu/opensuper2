"""For each function: message ids referenced (DAT_00494768 + off) -> text; plus callers."""
import re, json, sys, collections
msgs = {}
for l in open('/home/lizhaoliu/games/super2sp/dump/flow/msgs_bin5.txt', encoding='utf-8'):
    i, t = l.rstrip('\n').split(' ', 1); msgs[int(i, 16)] = t
D = '/home/lizhaoliu/games/super2sp/re/flow/decomp_full.c'
funcs = collections.OrderedDict(); cur = None
for line in open(D, encoding='utf-8', errors='replace'):
    m = re.match(r'// ==== (\S+) @ ([0-9a-f]+)', line)
    if m: cur = m.group(2); funcs[cur] = []; continue
    if cur: funcs[cur].append(line)
callers = collections.defaultdict(set)
for a, body in funcs.items():
    for c in set(re.findall(r'FUN_([0-9a-f]{8})', ''.join(body))):
        if c != a: callers[c].add(a)
want = sys.argv[1:] or [l.split()[0] for l in open('/home/lizhaoliu/games/super2sp/dump/flow/switches.txt') if not l.startswith('#')]
for a in sorted(set(want)):
    body = ''.join(funcs.get(a, []))
    ids = []
    for off in re.findall(r'DAT_00494768 \+ (0x[0-9a-f]+|\d+)\)', body):
        v = int(off, 0)
        if v % 4 == 0: ids.append(v // 4)
    for arr in re.findall(r'local_\w+\[\d\] = (0x[0-9a-f]+);', body): pass
    txt = ', '.join('%x:%s' % (i, msgs.get(i, '?')) for i in sorted(set(ids)))
    print('%s size=%d callers=%s\n    %s' % (a, len(body), ','.join(sorted(callers[a]))[:80], txt[:300]))
