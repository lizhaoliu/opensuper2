#!/bin/bash
# Original vs enhanced enemy AI (battle/ai_plus.rs) from natural turn-1 saves
# (the non-god s2auto runs ~/games/super2sp/dump/auto/m*-def-s*), the player
# side on 自動作戰. Writes OUT/results.txt (stage seed ai result turns lost)
# and prints the table.
#   tools/ai_eval.sh OUT            STAGES="2 3 .." SEEDS="1 2 3 4 5" JOBS=16
set -u
OUT=${1:?usage: tools/ai_eval.sh OUT}
REPO=$(cd "$(dirname "$0")/.." && pwd)
AUTO=${AUTO_DIR:-$HOME/games/super2sp/dump/auto}
GAME=${S2_GAME_DIR:-$HOME/games/super2sp/extracted/app}
STAGES=${STAGES:-"2 3 4 5 7 8 9 11 12 71 72 74 77 79 80 83 84"}
SEEDS=${SEEDS:-"1 2 3 4 5"}
mkdir -p "$OUT"; : > "$OUT/results.txt"
(cd "$REPO" && cargo build --release -q -p s2engine --bin s2auto) || exit 2
find_save() { for d in "$AUTO"/m*-def-s*; do for r in 0 1; do
    f=$(ls "$d/route$r"/turn1_*_"$(printf %03d "$1")".sav 2>/dev/null | head -1)
    [ -n "$f" ] && { echo "$f"; return; }; done; done; }
one() {
    local st=$1 seed=$2 ai=$3 f=$4 name=s${1}_${2}_$3
    timeout 900 "$REPO/target/release/s2auto" --game "$GAME" --out "$OUT" --from-save 1 --save-file "$f" \
        --max-stages 1 --retries 0 --seed "$seed" --run "$name" --enemy-ai "$ai" > "$OUT/$name.txt" 2>&1
    local line res turns lost
    line=$(grep -E '^ +0 +[0-9]+ ' "$OUT/$name.txt" | head -1)
    res=$(echo "$line" | awk '{print $5}'); turns=$(echo "$line" | awk '{print $4}')
    lost=$(cat "$OUT/$name"/route*.log 2>/dev/null | grep -c 'die UnitRef { side: Player')
    echo "$st $seed $ai ${res:-none} ${turns:-0} $lost" >> "$OUT/results.txt"
}
export -f one; export OUT REPO GAME
for st in $STAGES; do f=$(find_save "$st"); [ -z "$f" ] && continue
    for seed in $SEEDS; do for ai in original enhanced; do echo "$st $seed $ai $f"; done; done
done | xargs -P "${JOBS:-16}" -L 1 bash -c 'one "$@"' _
"$REPO/tools/.venv/bin/python" -I - "$OUT/results.txt" <<'PY'
import sys, collections
rows = [l.split() for l in open(sys.argv[1])]
by = collections.defaultdict(lambda: collections.defaultdict(list))
for st, seed, ai, res, turns, lost in rows:
    by[int(st)][ai].append((res, int(turns), int(lost)))
tot = {"original": [0, 0, 0, 0, 0], "enhanced": [0, 0, 0, 0, 0]}
print("| stage | original: player wins, turns, units lost | enhanced: player wins, turns, units lost |")
print("|---|---|---|")
for st in sorted(by):
    out = []
    for ai in ("original", "enhanced"):
        v = by[st][ai]; n = len(v); w = sum(1 for r in v if r[0] == "victory")
        t = sum(r[1] for r in v) / max(n, 1); l = sum(r[2] for r in v) / max(n, 1)
        u = sum(1 for r in v if r[0] not in ("victory", "defeat"))
        out.append(f"{w}/{n}, {t:.1f}, {l:.1f}" + (f" ({u} unfinished)" if u else ""))
        T = tot[ai]; T[0] += w; T[1] += n; T[2] += t * n; T[3] += l * n; T[4] += u
    print(f"| {st} | {out[0]} | {out[1]} |")
for ai, T in tot.items():
    if T[1]:
        print(f"{ai}: player wins {T[0]}/{T[1]} ({100*T[0]//T[1]}%), turns {T[2]/T[1]:.1f}, units lost {T[3]/T[1]:.1f}, unfinished {T[4]}")
PY
