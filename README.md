# OpenSuper2

A new engine for **超時空英雄傳說2 北方密使** (UserJoy, 1998) that plays the
game from your own copy of the original data files, the way ScummVM or
OpenRA do for their games. The aim is to reproduce the original faithfully
(graphics, rules, story and timing) while running natively on modern Windows
and Linux.

This repository contains no game content. Point the engine at a directory
holding the original files (`SUPERSP.PAL`, `SUPERSP.BIN`, `SUPERSP.CEL`,
`SUPERSP.PRG`, `SUPERSP\SUPERSP.DAT`, ...).

## Status

The whole game is ported from the original program's logic (every routine
cites the `FUN_xxxxxxxx` it reproduces; see `docs/coverage.md`): opening
movies, title, campaign story and events, battles with the original rules,
RNG and AI, magic, items, class changes, world map, towns, shops, church,
deployment, save/load (original `SAVE\UJnn.SAV` format), settings,
two-player battles and the ending; optional enemy AI variants and an enemy
stat boost are off by default.

Verification: the original program runs headlessly as an oracle
(`docs/oracle.md`) and is compared with the port pixel by pixel and save by
save. The regression gate `tools/oracle/check.sh` (41 scripts, 2135 shots) is
all identical; a sweep over every stage, story branch, cave, town visit and
two-player battlefield (`tools/oracle/sweep.sh`, 148 checks) has identical
RNG draws and saves everywhere and 146 checks identical shot for shot. The
few remaining frame differences and the per-area state are listed at the top
of `docs/battle-port-status.md`, `docs/flow-port-status.md`,
`docs/view-port-status.md` and `docs/qa-status.md`.

## Playing

Put `opensuper2.exe` next to the game files (`SUPERSP.PAL` etc.) and run
it, or pass `--game DIR`. Music plays through the system MIDI synthesizer on
Windows; elsewhere pass `--soundfont FILE.sf2`.

- Alt+Enter or F11: toggle fullscreen (remembered).
- `opensuper2.ini` (next to the game files): `fullscreen=0|1`,
  `scaling=fit|integer`, `window=W,H`, `fade_speed=1..16` (palette fades
  N times faster; 1 matches the original under 60 Hz page flipping — the
  original's fades are untimed, so many machines ran them faster),
  `ai=original|enhanced|tactical` and `enemy_boost=0..100` (not part of the
  original; `original` with boost 0 is exactly the original): `enhanced`
  keeps the original AI's decisions but varies the enemy's spell /
  technique / attack picks among near-best options and lets it buff its own
  side; `tactical` is a threat-map AI (focus fire, objective targets,
  positioning); `enemy_boost` adds N % to enemy HP/MP/ST and attack /
  defence / 劍術 / 戰技 on top of the original's per-stage bonus.
- Problems are written to `opensuper2.log` next to the game files.

## Layout

| Path | What |
|------|------|
| `crates/s2data` | readers for the original file formats |
| `crates/s2tool` | command-line dumpers (`s2tool`, `s2gfx`, `s2text`, `s2tables`, `s2audio`) |
| `crates/s2engine` | game logic, written against a small `Sys` (back buffer, clock, input); `s2run` headless runner, `s2auto` campaign harness |
| `crates/s2app` | desktop frontend (`opensuper2`): winit window, scaling, keyboard/mouse |
| `docs/` | format and behaviour notes (`formats*.md`, `rules.md`, `ai.md`, ...); port status (`*-port-status.md`, `qa-status.md`), oracle (`oracle.md`), coverage audit (`coverage.md`) |
| `re/symbols.tsv` | names given to functions of the original executable |
| `tools/oracle/` | the original program as a headless oracle: `check.sh` (regression gate), `sweep.sh`, `compare.py`, fixtures, scripts |
| `tools/` | reverse-engineering helpers: `unpack_pec2.py` (PECompact2 unpacker), `ghidra_scripts/`, `fn.sh`, `dis.py` / `disasm.py`, `jt.py`, `casemap.py`, `find_switches.py`, `copycalls.py`, `exestr.py`, `merge_decomp.py`; `ai_eval.sh` (enhanced AI evaluation) |
| `tools/explore/` | one-off format exploration scripts (kept for reference, see its README) |

## Build and run

```sh
cargo build --release
./target/release/opensuper2 --game /path/to/game
# Windows build from Linux:
cargo build --release -p s2app --target x86_64-pc-windows-gnu
```

Headless campaign harness (auto battle, crash/hang detection):

```sh
./target/release/s2auto --game /path/to/game --route both --max-stages 20
```

Headless run with scripted input and screenshots:

```sh
./target/release/s2run --game /path/to/game --out shots \
  --script "frames 60; shot title; key RETURN; frames 30; shot next"
```
