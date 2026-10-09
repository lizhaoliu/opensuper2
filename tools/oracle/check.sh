#!/bin/bash
# Regression check against the original game (docs/oracle.md):
#
#   tools/oracle/check.sh [SCRIPT.s2 ...]      (default: every tools/oracle/scripts/*.s2)
#
# Builds s2run, runs each script through s2run and the oracle (whose shots are
# cached by script, SAVE fixture, exe and oracle.py, so only the first run of a
# script is slow) and prints PASS/FAIL per shot. Exit status 0 when every shot
# of every script is pixel-identical, 1 otherwise, 2 on a build or setup error.
# Diff images: $S2_ORACLE_DIR/check/<script>/<shot>.diff.png (s2run | original | diff).
set -u
REPO=$(cd "$(dirname "$0")/../.." && pwd)
GAME=${S2_GAME_DIR:-$HOME/games/super2sp/extracted/app}
BASE=${S2_ORACLE_DIR:-$HOME/games/super2sp/dump/oracle}
PY="$REPO/tools/.venv/bin/python"
if [ $# -eq 0 ]; then set -- "$REPO"/tools/oracle/scripts/*.s2; fi
if ! (cd "$REPO" && cargo build --release -q -p s2engine --bin s2run); then
    echo "check: s2run does not build" >&2
    exit 2
fi
status=0
for script in "$@"; do
    name=$(basename "$script" .s2)
    out="$BASE/check/$name"
    mkdir -p "$out"
    # One run per script at a time: concurrent checks would share OUT and the
    # cache entry. The report goes to a per-run file (a redirection outside the
    # lock would truncate a running check's), then replaces check.txt.
    log="$out/check.$$.txt"
    flock "$out.lock" "$PY" -I "$REPO/tools/oracle/compare.py" --game "$GAME" --out "$out" \
        --script-file "$script" --cache "$BASE/cache" --log "$log" > /dev/null 2>&1
    rc=$?
    if [ $rc -gt 1 ]; then
        echo "check: $name: compare failed (see $out/check.txt)" >&2
        tail -5 "$log" >&2
        mv -f "$log" "$out/check.txt"
        status=2
        continue
    fi
    awk -v s="$name" '/ shots identical$/ {print s ": " $0; next}
        /^clocks part/ {print "note " s ": " $0; next}
        $2 == "identical" {print "PASS " s "/" $1; next}
        $1 == "file" && / identical/ {print "PASS " s "/" $2 " (file)"; next}
        {printf "FAIL %s/%s", s, $1; $1 = ""; print}' "$log"
    mv -f "$log" "$out/check.txt"
    [ $rc -ne 0 ] && [ $status -eq 0 ] && status=1
done
exit $status
