#!/bin/bash
# Broad differential sweep against the original (docs/qa-status.md): one
# `s2auto --oracle-check` per stage turn-1 save, per between-stages save
# (town tour + deployment) and per 雙人對戰 battlefield, run in parallel.
#
#   tools/oracle/sweep.sh saves        # make the s2auto runs the saves come from
#   tools/oracle/sweep.sh extra        # (part of saves) the branch runs to the rarer stages
#   tools/oracle/sweep.sh collect      # gather the saves from those runs
#   tools/oracle/sweep.sh run          # run every check not yet done (--force: all)
#   tools/oracle/sweep.sh table        # OUT/table.md from the reports
#   tools/oracle/sweep.sh all          # collect (if needed), run, table
#
# Environment: JOBS (20), FRAMES (4800 battle / VS), TOWN_FRAMES (8400),
# SHOT_EVERY (60), SWEEP_DIR (~/games/super2sp/dump/oracle/sweep),
# AUTO_DIR (s2auto runs to collect from, ~/games/super2sp/dump/auto),
# S2_GAME_DIR. Saves: SWEEP_DIR/saves/{battle_SSS.sav,between_SSS.sav};
# VS uses the battle_vs fixture (blue UJ01, red UJ02). Reports:
# SWEEP_DIR/runs/<name>/oracle-check.txt (+ compare.txt, cmp/ with the diff
# images and both programs' logs).
set -eu
REPO=$(cd "$(dirname "$0")/../.." && pwd)
GAME=${S2_GAME_DIR:-$HOME/games/super2sp/extracted/app}
SWEEP=${SWEEP_DIR:-$HOME/games/super2sp/dump/oracle/sweep}
AUTO=${AUTO_DIR:-$HOME/games/super2sp/dump/auto}
FIX=${S2_ORACLE_FIXTURES:-$HOME/games/super2sp/dump/oracle/fixtures}
JOBS=${JOBS:-20}
FRAMES=${FRAMES:-4800}
TOWN_FRAMES=${TOWN_FRAMES:-8400}
SHOT_EVERY=${SHOT_EVERY:-60}
PY="$REPO/tools/.venv/bin/python"
S2AUTO="$REPO/target/release/s2auto"
mkdir -p "$SWEEP/saves" "$SWEEP/runs"

collect() {
    # The newest turn-1 save of every stage found in the runs (s2auto copies
    # them as route<r>/turn1_<index>_<stage>.sav), and every 10th
    # between-stages save (S2_TEST_SAVE_KEEP=<dir>/between/r<r>).
    "$PY" -I - "$AUTO" "$SWEEP" <<'PY'
import os, re, shutil, sys
auto, sweep = sys.argv[1], sys.argv[2]
CAVES = {62, 63, 64, 65, 133, 134, 135}
RUNS = re.compile(os.environ.get("RUNS_RE", r"^sw2-"))
def allowed(run, st):
    # Saves of the sweep's own s2auto runs (tools/oracle/sweep.sh saves):
    # campaign and forced-choice runs for battles, sw2-cave-* for the caves
    # only (their cave-bearing stage starts with the protagonist moved).
    if not RUNS.search(run):
        return False
    if run.startswith("sw2-x"):
        # sw2-x<stage>[-<stage>..]: a branch run kept for the stages it names.
        return str(st) in run[5:].split("-")
    if "-cave-" in run:
        return st in CAVES
    return st not in CAVES
best = {}
for root, dirs, files in os.walk(auto):
    run = os.path.relpath(root, auto).split(os.sep)[0]
    for f in files:
        m = re.fullmatch(r"turn1_\d+_(\d+)\.sav", f)
        if m and allowed(run, int(m.group(1))):
            p = os.path.join(root, f)
            st = int(m.group(1))
            if st not in best or os.path.getmtime(p) > os.path.getmtime(best[st]):
                best[st] = p
for st, p in sorted(best.items()):
    shutil.copy(p, os.path.join(sweep, "saves", "battle_%03d.sav" % st))
between = {}
for root, dirs, files in os.walk(os.path.join(sweep, "between")):
    for f in sorted(files):
        m = re.fullmatch(r"between_(\d+)\.sav", f)
        if m:
            between.setdefault(int(m.group(1)), os.path.join(root, f))
picked = [st for i, st in enumerate(sorted(between)) if i % 10 == 0]
for st in picked:
    shutil.copy(between[st], os.path.join(sweep, "saves", "between_%03d.sav" % st))
# Saves made by hand for stages no script reaches (sweep/crafted, see extra()).
cr = os.path.join(sweep, "crafted")
if os.path.isdir(cr):
    for f in sorted(os.listdir(cr)):
        m = re.fullmatch(r"battle_(\d+)\.sav", f)
        if m:
            shutil.copy(os.path.join(cr, f), os.path.join(sweep, "saves", f))
            best.setdefault(int(m.group(1)), os.path.join(cr, f))
print("collected %d battle saves (stages %s) and %d between-stages saves (after %s)"
      % (len(best), ",".join(map(str, sorted(best))), len(picked), ",".join(map(str, picked))))
PY
}

jobs() {
    for f in "$SWEEP"/saves/battle_*.sav; do
        [ -e "$f" ] || continue
        n=$(basename "$f" .sav); echo "$n battle $FRAMES $f"
    done
    for f in "$SWEEP"/saves/between_*.sav; do
        [ -e "$f" ] || continue
        n=$(basename "$f" .sav); echo "$n town $TOWN_FRAMES $f"
    done
    if [ -d "$FIX/battle_vs" ]; then
        for k in 1 2 3 4 5 6 7 8 9 10; do echo "vs_$(printf %02d $k) vs:$k $FRAMES $FIX/battle_vs"; done
    fi
}

one() {
    # name mode frames save
    local name=$1 mode=$2 frames=$3 save=$4
    # Done already, from the same save (the fixture is the save's copy).
    if [ -z "${FORCE:-}" ] && [ -f "$SWEEP/runs/$name/oracle-check.txt" ]; then
        if [ -d "$save" ] || cmp -s "$save" "$SWEEP/runs/$name/fixture/UJ01.SAV"; then
            return 0
        fi
    fi
    "$S2AUTO" --game "$GAME" --out "$SWEEP/runs" --run "$name" --oracle-check "$save" \
        --oracle-mode "$mode" --frames "$frames" --shot-every "$SHOT_EVERY" > "$SWEEP/runs/$name.log" 2>&1 \
        || echo "$name: s2auto exited $?" >> "$SWEEP/runs/$name.log"
    echo "$name: $(grep SUMMARY "$SWEEP/runs/$name/oracle-check.txt" 2>/dev/null || echo 'no report')"
}
export -f one
export GAME SWEEP S2AUTO SHOT_EVERY

run() {
    (cd "$REPO" && cargo build --release --bin s2auto --bin s2run 2>&1 | tail -1)
    jobs | xargs -P "$JOBS" -L 1 bash -c 'one "$@"' _
}

table() {
    "$PY" -I - "$SWEEP" <<'PY'
import os, re, sys
sweep = sys.argv[1]
rows = []
for name in sorted(os.listdir(os.path.join(sweep, "runs"))):
    p = os.path.join(sweep, "runs", name, "oracle-check.txt")
    if not os.path.isfile(p):
        continue
    text = open(p).read()
    m = re.search(r"SUMMARY frames=(\d+) shots=(\d+)/(\d+) draws_identical=(\w+) draws=\((.*?)\) saves_identical=(\w+) first_diff=(\S+)", text)
    if not m:
        rows.append((name, "?", "?", "?", "?", "no summary"))
        continue
    frames, same, total, dsame, draws, saves, first = m.groups()
    d = re.search(r"port (\d+), original (\d+)", draws)
    same_draws = dsame == "true" and d is not None and d.group(1) == d.group(2)
    dcol = ("%s/%s" % (d.group(2), d.group(1)) if d else draws) + ("" if same_draws else (" prefix" if dsame == "true" else " ✗"))
    fd = re.search(r"first differing draw #(\d+)", text)
    note = first if first != "-" else ""
    if fd:
        note += (" · " if note else "") + "draw #" + fd.group(1)
    rows.append((name, frames, "%s/%s" % (same, total), dcol, "yes" if saves == "true" else "no", note))
out = ["| check | frames | shots identical | draws (original/port) | saves identical | first divergence |",
       "|---|---|---|---|---|---|"]
out += ["| %s | %s | %s | %s | %s | %s |" % r for r in rows]
ok = sum(1 for r in rows if r[2].split("/")[0] == r[2].split("/")[-1] and "✗" not in r[3] and "prefix" not in r[3] and r[4] == "yes")
out.append("")
out.append("%d checks, %d fully identical (shots, draws, saves)." % (len(rows), ok))
open(os.path.join(sweep, "table.md"), "w").write("\n".join(out) + "\n")
print("\n".join(out))
PY
}

saves() {
    # The s2auto runs collect takes its saves from (turn-1 autosaves with the
    # harness' AI-mode helpers undone): both routes to the end (god mode,
    # seeds 2, 3), the story branches to stages 33/35/36/102 (--choice) and
    # the caves (--caves). Branch and cave runs start from turn-1 saves of
    # stages 29/31/59/99/105/129 the campaign runs produce.
    local G="$GAME" A="$AUTO" B="$S2AUTO"
    for s in 2 3; do
        [ -z "${SAVES_FORCE:-}" ] && [ -f "$A/sw2-god-s$s/summary.txt" ] && continue
        # S2_TEST_SAVE_KEEP (flow/town.rs): every between-stages arrival also
        # leaves between_<stage>.sav, the town segments' saves.
        S2_TEST_SAVE=40 S2_TEST_SAVE_KEEP="$SWEEP/between/s$s" \
        "$B" --game "$G" --out "$A" --route both --max-stages 200 --recover 3 --timeout-secs 90 \
            --random-choices --god --seed $s --run sw2-god-s$s > "$A/sw2-god-s$s.txt" 2>&1 &
    done
    wait
    # The branch / cave runs start from a turn-1 save of the stage before
    # the branch: from these runs, else from any earlier s2auto run.
    find_save() { { ls "$A"/sw2-god-s*/route$1/turn1_*_$(printf %03d $2).sav; ls "$A"/*/route$1/turn1_*_$(printf %03d $2).sav; } 2>/dev/null | head -1; }
    one_run() { local name=$1 r=$2 st=$3; shift 3; local f; f=$(find_save $r $st)
        [ -n "$f" ] || { echo "$name: no turn-1 save of stage $st"; return; }
        "$B" --game "$G" --out "$A" --route $r --god --seed 1 --from-save 51 --save-file "$f" --timeout-secs 90 \
            "$@" --run $name > "$A/$name.txt" 2>&1; }
    one_run sw2-br29 0 29 --choice 29:1 --max-stages 4 &
    one_run sw2-br31 0 31 --choice 31:2 --max-stages 4 &
    one_run sw2-br99 1 99 --choice 99:1 --max-stages 4 &
    one_run sw2-cave-62 0 59 --caves --max-stages 2 --retries 0 &
    one_run sw2-cave-133 1 129 --caves --max-stages 2 --retries 0 &
    one_run sw2-cave-134 1 105 --caves --max-stages 2 --retries 0 &
    one_run sw2-cave-63 0 29 --caves --choice 29:1 --max-stages 3 --retries 0 &
    one_run sw2-cave-65 0 31 --caves --choice 31:2 --set-flag 36:4 --max-stages 4 --retries 0 &
    one_run sw2-cave-135 1 99 --caves --choice 99:1 --max-stages 3 --retries 0 &
    wait
}

extra() {
    # Stages no campaign run reaches, each from the turn-1 save of the stage
    # that branches to it (story flags / choices from its event records,
    # docs/qa-status.md); 108 and 128 from hand-made saves (below).
    local G="$GAME" A="$AUTO" B="$S2AUTO"
    find_save() { { ls "$A"/sw2-god-s*/route$1/turn1_*_$(printf %03d $2).sav; ls "$A"/*/route$1/turn1_*_$(printf %03d $2).sav; } 2>/dev/null | head -1; }
    x() { local name=$1 r=$2 st=$3 n=$4; shift 4; local f; f=$(find_save $r $st)
        [ -n "$f" ] || { echo "$name: no turn-1 save of stage $st"; return; }
        "$B" --game "$G" --out "$A" --route $r --god --seed 1 --from-save 51 --save-file "$f" --timeout-secs 90 \
            --retries 1 --max-stages $n "$@" --run $name > "$A/$name.txt" 2>&1; }
    x sw2-x15 0 12 2 --choice 12:1 &
    x sw2-x21 0 17 2 --choice 17:2 &
    x sw2-x43 0 41 2 --set-flag 41:16 &
    x sw2-x44 0 41 2 --set-flag 41:7 &
    x sw2-x50 0 48 2 --choice 48:1 &
    x sw2-x51 0 56 2 --set-flag 56:47 --set-flag 56:48=0 --set-flag 56:49=0 --set-flag 56:50=0 --choice 56:1 &
    x sw2-x57 0 56 2 --set-flag 56:49 --set-flag 56:47=0 --set-flag 56:48=0 --choice 56:1 &
    x sw2-x55 0 54 2 --set-flag 54:54 &
    x sw2-x64 0 46 2 --set-flag 46:14 &
    x sw2-x82 1 79 3 --choice 162:2 &
    x sw2-x93 1 92 2 --set-flag 92:66=0 &
    x sw2-x103-104 1 100 3 --choice 100:1 &
    x sw2-x108 1 89 2 --set-flag 89:66 --choice 89:2 &
    x sw2-x118 1 117 2 --choice 117:1 &
    x sw2-x121 1 126 2 --choice 126:1 &
    x sw2-x125 1 124 2 --set-flag 124:54 &
    x sw2-x132 1 96 2 --set-flag 96:64 --set-flag 96:65=0 &
    wait
    # 108 殺之宴: stage 89's script sets next 108 at turn 1 and the victory
    # choice always overrides it (90 / 92), so no play reaches it; 128
    # 未拼場景關 has no predecessor. Both from a between-stages save with the
    # next stage (campaign block 0x1a9 + 0x10) changed by hand; both stall
    # (s2auto kills them), so the turn-1 autosave is taken from the run's SAVE
    # with the protagonist's helper mode 6 set back to 1.
    local C="$SWEEP/crafted" t
    mkdir -p "$C"
    for t in 108:089 128:126; do
        local st=${t%%:*} from=${t#*:}
        local bf; bf=$(ls "$SWEEP"/between/*/between_$from.sav 2>/dev/null | head -1)
        [ -n "$bf" ] || { echo "sw2-x$st: no between-stages save after stage $from"; continue; }
        "$PY" -I - "$bf" "$C/to$st.sav" "$st" <<'PY2'
import struct, sys
d = bytearray(open(sys.argv[1], "rb").read())
struct.pack_into("<i", d, 9 + 0x32 + 0xff + 0x50 + 0x1f + 0x10, int(sys.argv[3]))
open(sys.argv[2], "wb").write(d)
PY2
        "$B" --game "$G" --out "$A" --route 1 --god --seed 1 --from-save 1 --save-file "$C/to$st.sav" \
            --max-stages 1 --retries 0 --timeout-secs 90 --run sw2-x$st > "$A/sw2-x$st.txt" 2>&1
        "$PY" -I - "$A"/sw2-x$st/game1/SAVE/UJ51.SAV "$C/battle_$st.sav" <<'PY2'
import sys
d = bytearray(open(sys.argv[1], "rb").read())
for k in range(20):
    o = 0x28a + k * 0x16a
    if d[o + 2:o + 4] == b"\x01\x00" and d[o + 0xa:o + 0xc] == b"\x06\x00":
        d[o + 0xa:o + 0xc] = b"\x01\x00"
open(sys.argv[2], "wb").write(d)
PY2
    done
}

case "${1:-all}" in
    saves) saves; extra ;;
    extra) extra ;;
    collect) collect ;;
    run) [ "${2:-}" = "--force" ] && export FORCE=1; run ;;
    table) table ;;
    all) [ -n "$(ls "$SWEEP"/saves 2>/dev/null)" ] || collect; run; table ;;
    *) echo "usage: $0 saves|extra|collect|run [--force]|table|all" >&2; exit 2 ;;
esac
