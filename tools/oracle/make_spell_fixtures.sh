#!/bin/bash
# Create the SAVE fixture of the spell oracle scripts (tools/oracle/scripts/spells_*.s2),
# unless present (--force remakes it):
#   spells5/UJ51.SAV   route 0, the turn-1 autosave of stage 5 (黃沙烈燄): 羅莎莉 / 南巡使 cast
#                      magic, 志超 / 刑天 / 秋月 sword techniques and combos; loaded as slot 51.
# s2auto (route 0, seed 1, no --god) keeps the turn-1 autosave of every battle as
# turn1_<index>_<stage>.sav. The save is made by the port; both programs load the same file.
set -eu
REPO=$(cd "$(dirname "$0")/../.." && pwd)
GAME=${S2_GAME_DIR:-$HOME/games/super2sp/extracted/app}
FIX=${S2_ORACLE_FIXTURES:-$HOME/games/super2sp/dump/oracle/fixtures}
if [ "${1:-}" != "--force" ] && [ -f "$FIX/spells5/UJ51.SAV" ]; then
    echo "spell fixtures present in $FIX"
    exit 0
fi
cd "$REPO" && cargo build --release -q -p s2engine --bin s2auto
tmp=$(mktemp -d)
target/release/s2auto --game "$GAME" --out "$tmp" --route 0 --max-stages 6 --run sp >/dev/null || true
mkdir -p "$FIX/spells5"
cp "$tmp/sp/route0/turn1_006_005.sav" "$FIX/spells5/UJ51.SAV"
rm -rf "$tmp"
echo "spell fixtures written to $FIX"
