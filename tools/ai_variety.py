"""Enemy spell/technique variety per AI mode, from an ai_eval.sh output dir.

Usage: tools/.venv/bin/python -I tools/ai_variety.py OUT

Reads OUT/s<stage>_<seed>_<ai>/route*.log battle traces ("[battle] cast
UnitRef { side: Enemy, idx: N } spell S ...") and prints, per AI mode: enemy
casts, distinct spells cast, and the mean number of distinct spells per
casting enemy unit per battle (how often a unit varies what it casts).
"""
import collections
import glob
import os
import re
import sys

CAST = re.compile(r"\[battle\] cast UnitRef \{ side: Enemy, idx: (\d+) \} spell (\d+)")

def main(out):
    stats = collections.defaultdict(lambda: {"casts": 0, "spells": collections.Counter(), "per_unit": []})
    for d in sorted(glob.glob(os.path.join(out, "s*_*_*"))):
        if not os.path.isdir(d):
            continue
        ai = os.path.basename(d).rsplit("_", 1)[1]
        units = collections.defaultdict(set)
        for log in glob.glob(os.path.join(d, "route*.log")):
            for line in open(log, errors="replace"):
                m = CAST.search(line)
                if m:
                    u, sp = int(m.group(1)), int(m.group(2))
                    stats[ai]["casts"] += 1
                    stats[ai]["spells"][sp] += 1
                    units[u].add(sp)
        stats[ai]["per_unit"].extend(len(v) for v in units.values())
    print("| AI | enemy casts | distinct spells | distinct spells per casting unit | top spell share |")
    print("|---|---|---|---|---|")
    for ai, s in sorted(stats.items()):
        pu = s["per_unit"]
        top = s["spells"].most_common(1)[0][1] / s["casts"] if s["casts"] else 0
        mean = sum(pu) / len(pu) if pu else 0
        print(f"| {ai} | {s['casts']} | {len(s['spells'])} | {mean:.2f} | {top:.0%} |")

if __name__ == "__main__":
    main(sys.argv[1])
