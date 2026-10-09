#!/bin/bash
# Create the late-game SAVE fixtures of the flow oracle scripts, unless present
# (--force remakes them; the scripts' recorded inputs and click positions belong to
# the stored fixtures, so a remade fixture may need its script re-recorded):
#   classchange          route 0, between stages after stage 25 (six members may change class)
#   classchange_hidden   route 0, after stage 48, 法科爾 holding 銀牙彎刀 (equip slot 5) for 海盜
#   cave                 route 0, stage-59 turn-1 autosave with the cave-found byte set
#   cave_return          the cave stage 62's turn-1 autosave as cave.s2 writes it, five 魔頭
#                        dead (DEAD set, DEPLOYED cleared) and the last at 1 HP
#   ending_r0/ending_r1  the last stage's (60 / 130) turn-1 autosave with the turn set to 15
#   ending_74/ending_80  ending_r0 with flag 42 set, 43 cleared / ending_r1 with 63 set, 64, 65 cleared
#   ending_73/75/77/79   stage 61 / 131's turn-1 autosave (s2auto --set-flag 60:15 / 130:15 from
#                        the last stage's) with story flag 4 set (the stage is over at once) and
#                        flag 42 / 43 (route 0), 65 / 63 (route 1) as the only branch flag
# One s2auto run per route (seed 1, --god --recover 3) with S2_TEST_SAVE=1 and
# S2_TEST_SAVE_KEEP (flow/town.rs) keeps every between-stages save as between_<stage>.sav;
# its turn-1 autosaves are turn1_<index>_<stage>.sav.
set -eu
REPO=$(cd "$(dirname "$0")/../.." && pwd)
GAME=${S2_GAME_DIR:-$HOME/games/super2sp/extracted/app}
FIX=${S2_ORACLE_FIXTURES:-$HOME/games/super2sp/dump/oracle/fixtures}
NAMES="classchange classchange_hidden cave cave_return ending_r0 ending_r1 ending_73 ending_74 ending_75 ending_77 ending_79 ending_80"
if [ "${1:-}" != "--force" ]; then
    missing=0
    for n in $NAMES; do [ -f "$FIX/$n/UJ01.SAV" ] || missing=1; done
    if [ $missing = 0 ]; then
        echo "late fixtures present in $FIX"
        exit 0
    fi
fi
cd "$REPO" && cargo build --release -q -p s2engine --bin s2auto --bin s2run
tmp=$(mktemp -d)
for r in 0 1; do
    S2_TEST_SAVE=1 S2_TEST_SAVE_KEEP="$tmp/keep$r" target/release/s2auto --game "$GAME" --out "$tmp" \
        --route $r --god --recover 3 --retries 10 --max-stages 200 --run r$r >/dev/null &
done
wait
turn1() {  # turn1 ROUTE STAGE -> the first turn-1 autosave of that stage
    ls "$tmp/r$1/route$1"/turn1_*_"$(printf %03d "$2")".sav | head -1
}
patch() {  # patch SRC DST WHAT: a copy of SRC with one edit (save layout: flow/save.rs)
    mkdir -p "$(dirname "$2")"
    "$REPO/tools/.venv/bin/python" -I - "$1" "$2" "$3" "$GAME" <<'EOF'
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
if what == "cave":
    d[C + 0x1a] = 1
elif what == "turn15":
    struct.pack_into("<i", d, C + 0xc, 15)
elif what == "hidden":
    first, n, _ = army(P)
    u = next(first + k * 0x16A for k in range(n) if struct.unpack_from("<h", d, first + k * 0x16A + 2)[0] == 32)
    struct.pack_into("<H", d, u + 0xcb + 10, 266)  # 法科爾 (char 32) holds 銀牙彎刀
elif what == "weak":
    _, _, e = army(P)
    first, n, _ = army(e)
    for k in range(n):
        u = first + k * 0x16A
        if k < n - 1:
            f = struct.unpack_from("<I", d, u + 0x30)[0]
            struct.pack_into("<I", d, u + 0x30, (f & ~0x80000000) | 0x20000000)
            struct.pack_into("<h", d, u + 0x124, 0)
        else:
            struct.pack_into("<h", d, u + 0x124, 1)
elif what.startswith("flags:"):  # flags:+4,+42,-43 sets / clears story flags (block A, flag n at n - 1)
    A = 9 + 0x32 + 0xff
    for f in what[6:].split(","):
        d[A + int(f[1:]) - 1] = 1 if f[0] == "+" else 0
elif what != "copy":
    sys.exit("unknown edit " + what)
open(dst, "wb").write(d)
EOF
}
patch "$tmp/keep0/between_025.sav" "$FIX/classchange/UJ01.SAV" copy
patch "$tmp/keep0/between_048.sav" "$FIX/classchange_hidden/UJ01.SAV" hidden
patch "$(turn1 0 59)" "$FIX/cave/UJ01.SAV" cave
patch "$(turn1 0 60)" "$FIX/ending_r0/UJ01.SAV" turn15
patch "$(turn1 1 130)" "$FIX/ending_r1/UJ01.SAV" turn15
patch "$FIX/ending_r0/UJ01.SAV" "$FIX/ending_74/UJ01.SAV" flags:+42,-43
patch "$FIX/ending_r1/UJ01.SAV" "$FIX/ending_80/UJ01.SAV" flags:+63,-64,-65
# Stage 61 / 131: 克雷布特里 falls at once in the last stage (flag 15), the next stage's autosave.
for r in 0 1; do
    s=$([ $r = 0 ] && echo 60 || echo 130)
    target/release/s2auto --game "$GAME" --out "$tmp" --route $r --god --from-save 1 \
        --save-file "$(turn1 $r $s)" --set-flag $s:15 --max-stages 2 --retries 0 --run b$r >/dev/null || true
done
b61=$(ls "$tmp/b0/route0"/turn1_*_061.sav | head -1)
b131=$(ls "$tmp/b1/route1"/turn1_*_131.sav | head -1)
patch "$b61" "$FIX/ending_73/UJ01.SAV" flags:+4,+42,-43
patch "$b61" "$FIX/ending_75/UJ01.SAV" flags:+4,+43,-42
patch "$b131" "$FIX/ending_79/UJ01.SAV" flags:+4,+63,-64,-65
patch "$b131" "$FIX/ending_77/UJ01.SAV" flags:+4,+65,-63
# cave_return: the cave stage's turn-1 autosave (UJ51) as s2run writes it running cave.s2.
"$REPO/tools/.venv/bin/python" -I "$REPO/tools/oracle/compare.py" --game "$GAME" --out "$tmp/cave" \
    --script-file "$REPO/tools/oracle/scripts/cave.s2" --port-only >/dev/null
patch "$tmp/cave/game/SAVE/UJ51.SAV" "$FIX/cave_return/UJ01.SAV" weak
rm -rf "$tmp"
echo "late fixtures written to $FIX"
