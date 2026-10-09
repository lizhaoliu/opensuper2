#!/bin/bash
# Create the SAVE fixtures the oracle scripts load ("#! saves NAME"), unless present:
#   after1/UJ01.SAV  between stages 1 and 2 (world map, march, deployment)
#   town/UJ01.SAV    between stages 2 and 3 (a town)
#   revive/UJ01.SAV  a between-stages town save with a dead member (the stored
#                    one: after stage 4, two dead; made before the disc fix, so
#                    its disc field, 0x26a + 0x10, was set to 1 by hand)
#   empty/           no saves (scripts that must not depend on the SAVE folder)
#   ending/UJ01.SAV  the original's own stage-1 turn-1 autosave (stage1.s2 up
#                    to the first player phase, under the oracle) with the next
#                    stage set to 0 (campaign block 0x1a9 + 0x10): winning it
#                    runs the ending
# s2auto (route 0, seed 1) plays stages 1-5; S2_TEST_SAVE=20 makes the port write
# slot 20 + i on its i-th arrival between stages (flow/town.rs). The saves are
# made by the port; both programs then load the same file. Existing fixtures
# are kept (a newer port plays the stages differently, so remade files differ);
# --force remakes them all, which changes every script's oracle cache key.
set -eu
REPO=$(cd "$(dirname "$0")/../.." && pwd)
GAME=${S2_GAME_DIR:-$HOME/games/super2sp/extracted/app}
FIX=${S2_ORACLE_FIXTURES:-$HOME/games/super2sp/dump/oracle/fixtures}
mkdir -p "$FIX/empty"
if [ ! -f "$FIX/ending/UJ01.SAV" ]; then
    tmp=$(mktemp -d)
    script=$(sed '/^shot p28$/q' "$REPO/tools/oracle/scripts/stage1.s2")
    "$REPO/tools/.venv/bin/python" -I "$REPO/tools/oracle/oracle.py" --game "$GAME" --saves "$FIX/empty" \
        --out "$tmp" --script "$script" >/dev/null
    mkdir -p "$FIX/ending"
    "$REPO/tools/.venv/bin/python" -I - "$tmp/scratch/SAVE/UJ51.SAV" "$FIX/ending/UJ01.SAV" <<'PY'
import struct, sys
d = bytearray(open(sys.argv[1], "rb").read())
struct.pack_into("<i", d, 9 + 0x32 + 0xff + 0x50 + 0x1f + 0x10, 0)
open(sys.argv[2], "wb").write(d)
PY
    rm -rf "$tmp"
fi
if [ "${1:-}" != "--force" ] && [ -f "$FIX/after1/UJ01.SAV" ] && [ -f "$FIX/town/UJ01.SAV" ] \
    && [ -f "$FIX/revive/UJ01.SAV" ]; then
    echo "fixtures present in $FIX"
    exit 0
fi
cd "$REPO" && cargo build --release -q -p s2engine --bin s2auto
tmp=$(mktemp -d)
S2_TEST_SAVE=20 target/release/s2auto --game "$GAME" --out "$tmp" --route 0 --max-stages 6 --run fx >/dev/null 2>&1
mkdir -p "$FIX/after1" "$FIX/town" "$FIX/revive"
cp "$tmp/fx/game0/SAVE/UJ20.SAV" "$FIX/after1/UJ01.SAV"
cp "$tmp/fx/game0/SAVE/UJ21.SAV" "$FIX/town/UJ01.SAV"
# revive: the first between-stages save with a dead member (none: keep the old one).
"$REPO/tools/.venv/bin/python" -I - "$tmp/fx/game0/SAVE" "$FIX/revive" "$GAME" <<'PY'
import os, shutil, struct, sys
src, dst, game = sys.argv[1:4]
items = [n for n in os.listdir(game) if n.upper() == "ITEMSP.BIN"][0]
n_items = struct.unpack("<h", open(os.path.join(game, items), "rb").read(2))[0]
for k in range(20, 51):
    p = os.path.join(src, "UJ%02d.SAV" % k)
    if not os.path.exists(p):
        break
    d = open(p, "rb").read()
    o = 9 + 0x32 + 0xff + 0x50 + 0x1f + 0x1d + 4 + 0xa0 + 0x1c
    n = struct.unpack_from("<I", d, o)[0]
    dead = [i for i in range(n) if struct.unpack_from("<I", d, o + 4 + i * 0x16a + 0x30)[0] & 0x20000000]
    if dead:
        shutil.copyfile(p, os.path.join(dst, "UJ01.SAV"))
        break
else:
    print("no save with a dead member: revive/ left as it was", file=sys.stderr)
PY
rm -rf "$tmp"
echo "fixtures written to $FIX"
