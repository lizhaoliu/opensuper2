#!/bin/bash
# Create the SAVE fixtures of the battle oracle scripts (tools/oracle/scripts/battle_*.s2),
# unless present (--force remakes them):
#   battle_r0_011 / _030 / _050 / _060   route 0, the turn-1 autosave of that stage (UJ01.SAV)
#   battle_r1_077 / _100 / _118 / _136   route 1, likewise (136 is a hidden stage)
#   battle_vs                            UJ01 = the stage-30 save, UJ02 = the stage-100 save (雙人對戰)
# s2auto (seed 1, --god so the party survives, --recover after stalls) keeps the turn-1
# autosave of every battle as turn1_<index>_<stage>.sav; the first one of each stage is used.
# The saves are made by the port; both programs then load the same file.
set -eu
REPO=$(cd "$(dirname "$0")/../.." && pwd)
GAME=${S2_GAME_DIR:-$HOME/games/super2sp/extracted/app}
FIX=${S2_ORACLE_FIXTURES:-$HOME/games/super2sp/dump/oracle/fixtures}
NAMES="battle_r0_011 battle_r0_030 battle_r0_050 battle_r0_060 battle_r1_077 battle_r1_100 battle_r1_118 battle_r1_136"
if [ "${1:-}" != "--force" ]; then
    missing=0
    for n in $NAMES; do [ -f "$FIX/$n/UJ01.SAV" ] || missing=1; done
    [ -f "$FIX/battle_vs/UJ02.SAV" ] || missing=1
    if [ $missing = 0 ]; then
        echo "battle fixtures present in $FIX"
        exit 0
    fi
fi
cd "$REPO" && cargo build --release -q -p s2engine --bin s2auto
tmp=$(mktemp -d)
target/release/s2auto --game "$GAME" --out "$tmp" --route both --god --recover 3 --retries 10 \
    --max-stages 200 --run fx >/dev/null
pick() {  # pick ROUTE STAGE -> path of the first turn-1 save of that stage
    ls "$tmp/fx/route$1"/turn1_*_"$(printf %03d "$2")".sav | head -1
}
for n in $NAMES; do
    r=${n:8:1}
    s=$((10#${n:10:3}))
    mkdir -p "$FIX/$n"
    cp "$(pick "$r" "$s")" "$FIX/$n/UJ01.SAV"
done
mkdir -p "$FIX/battle_vs"
cp "$(pick 0 30)" "$FIX/battle_vs/UJ01.SAV"
cp "$(pick 1 100)" "$FIX/battle_vs/UJ02.SAV"
rm -rf "$tmp"
echo "battle fixtures written to $FIX"
