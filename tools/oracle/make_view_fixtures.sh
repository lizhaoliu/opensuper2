#!/bin/bash
# Create the SAVE fixtures of the view coverage scripts, unless present (--force
# remakes them). Each is the spells5 fixture (route 0, stage 5 黃沙烈燄 turn-1
# autosave, make_spell_fixtures.sh) with unit records edited (save layout:
# flow/save.rs, unit record docs/structs.md), loaded as slot 51:
#   status5   statuses play does not reach this early, all permanent (turns -1):
#               志超 (P0) 隱身 invisible   飛霜 (P1) 石化 petrified   艾文 (P2) 凍結 frozen
#               羅莎莉 (P3) 中毒 poison     沙盜弩手 E0 隱身, E1 石化, E2 凍結
#   summon5   羅莎莉 (聖徒, 喚術) knows 地元素 (learned[6] bit 0) with full MP
#   cast5     羅莎莉 (P3) knowing 隱身 (learned[4] bit 4) with full MP; 刑天 (P5) and 秋月 (P6) knowing 長虹劍法 and
#             裂地劍法 (learned[9] bits 1, 3), 潮雨劍法 and 四象劍法 (learned[10] bits 3, 4)
#             with full ST
#   summondead5  the turn-2 autosave (UJ52) s2run writes running view_summon5.s2 up to its
#             first turn-2 shot, with the summoned 地元素 (P18) moved next to the archer
#             E0 at (17, 13) and left at 1 HP (play rarely kills a summon this early)
set -eu
REPO=$(cd "$(dirname "$0")/../.." && pwd)
GAME=${S2_GAME_DIR:-$HOME/games/super2sp/extracted/app}
FIX=${S2_ORACLE_FIXTURES:-$HOME/games/super2sp/dump/oracle/fixtures}
NAMES="status5 summon5 cast5 summondead5"
if [ "${1:-}" != "--force" ]; then
    missing=0
    for n in $NAMES; do [ -f "$FIX/$n/UJ51.SAV" ] || missing=1; done
    if [ $missing = 0 ]; then
        echo "view fixtures present in $FIX"
        exit 0
    fi
fi
"$REPO/tools/oracle/make_spell_fixtures.sh"
patch() {  # patch NAME [SRC]: a copy of SRC (default spells5/UJ51.SAV) with the NAME edits
    mkdir -p "$FIX/$1"
    "$REPO/tools/.venv/bin/python" -I - "${2:-$FIX/spells5/UJ51.SAV}" "$FIX/$1/UJ51.SAV" "$1" "$GAME" <<'EOF'
import glob, struct, sys
src, dst, what, game = sys.argv[1:]
d = bytearray(open(src, "rb").read())
C = 9 + 0x32 + 0xff + 0x50 + 0x1f            # campaign block
P = C + 0x1d + 4 + 0xa0 + 0x1c               # player army: u32 n, n units of 0x16a
items = [p for p in glob.glob(game + "/*") if p.upper().endswith("ITEMSP.BIN")][0]
n_items = struct.unpack("<h", open(items, "rb").read(2))[0]
def army(p):
    n = struct.unpack_from("<I", d, p)[0]
    return p + 4, n, p + 4 + n * 0x16A + 4 + 4 + 4 + 2 + 4 + n_items + 0x21
pf, pn, e = army(P)
ef, en, _ = army(e)
unit = lambda first, k: first + k * 0x16A
def status(u, bit):                          # status bit, permanent (+0x85 status_turns[i] = -1)
    struct.pack_into("<I", d, u + 0x3c, struct.unpack_from("<I", d, u + 0x3c)[0] | bit)
    d[u + 0x85 + bit.bit_length() - 1] = 0xff
def buff_b(u, bit, k, half):                 # buffs bit with buff_b[k] (turns, d_atk, d_def)
    struct.pack_into("<I", d, u + 0x38, struct.unpack_from("<I", d, u + 0x38)[0] | bit)
    atk, df = struct.unpack_from("<hh", d, u + 0xc3)
    struct.pack_into("<Bhh", d, u + 0x63 + 5 * k, 0xff, -(atk // half), -(df // half))
if what == "status5":
    status(unit(pf, 0), 0x10)                # invisible
    buff_b(unit(pf, 1), 0x200, 0, 2)         # petrified: ATK/DEF -1/2
    status(unit(pf, 2), 0x2000)              # frozen
    buff_b(unit(pf, 3), 0x400, 1, 5)         # poison: ATK/DEF -1/5
    status(unit(ef, 0), 0x10)
    buff_b(unit(ef, 1), 0x200, 0, 2)
    status(unit(ef, 2), 0x2000)
elif what == "summon5":
    u = unit(pf, 3)                          # 羅莎莉
    learned = struct.unpack_from("<H", d, u + 0xfb + 2 * 6)[0]
    struct.pack_into("<H", d, u + 0xfb + 2 * 6, learned | 1)
    struct.pack_into("<h", d, u + 0x126, struct.unpack_from("<h", d, u + 0xb9)[0])
elif what == "cast5":
    def learn(u, school, bits):
        o = u + 0xfb + 2 * school
        struct.pack_into("<H", d, o, struct.unpack_from("<H", d, o)[0] | bits)
    u = unit(pf, 3)                          # 羅莎莉
    learn(u, 4, 1 << 4)
    struct.pack_into("<h", d, u + 0x126, struct.unpack_from("<h", d, u + 0xb9)[0])
    for k in (5, 6):                         # 刑天, 秋月
        u = unit(pf, k)
        learn(u, 9, (1 << 1) | (1 << 3))
        learn(u, 10, (1 << 3) | (1 << 4))
        struct.pack_into("<h", d, u + 0x128, struct.unpack_from("<h", d, u + 0xbb)[0])
elif what == "summondead5":
    u = unit(pf, 18)                         # 地元素
    assert struct.unpack_from("<I", d, u + 0x30)[0] & 0x10000000
    struct.pack_into("<ii", d, u + 0x23, 17, 13)
    struct.pack_into("<h", d, u + 0x124, 1)
else:
    sys.exit("unknown edit " + what)
open(dst, "wb").write(d)
EOF
}
patch status5
patch summon5
patch cast5
# summondead5: s2run's own turn-2 autosave of the summon run.
tmp=$(mktemp -d)
awk '{print} /^shot t2_01$/ {print "quit"; exit}' "$REPO/tools/oracle/scripts/view_summon5.s2" > "$tmp/t2.s2"
cd "$REPO" && cargo build --release -q -p s2engine --bin s2run
S2_ORACLE_FIXTURES="$FIX" "$REPO/tools/.venv/bin/python" -I "$REPO/tools/oracle/compare.py" --game "$GAME" \
    --out "$tmp/run" --script-file "$tmp/t2.s2" --port-only >/dev/null
patch summondead5 "$tmp/run/game/SAVE/UJ52.SAV"
rm -rf "$tmp"
echo "view fixtures written to $FIX"
