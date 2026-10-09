# Ground-truth oracle

`tools/oracle/` runs the original program headlessly and compares it with
the port, shot by shot, from the same s2run script.

## Regression check (one command)

```sh
tools/oracle/check.sh                              # every tools/oracle/scripts/*.s2
tools/oracle/check.sh tools/oracle/scripts/town.s2 # one script
```

It builds s2run, runs each script through s2run and the oracle, and prints
`PASS script/shot` or `FAIL script/shot N px differ in boxes …` per shot, a
summary per script, and `note script: clocks part at frame N` when the two
virtual clocks first disagree. The exit status is 0 only when every shot is
pixel-identical (1 on a difference, 2 on a build/setup error). Diff images:
`~/games/super2sp/dump/oracle/check/<script>/<shot>.diff.png` (s2run |
original | differences in magenta).

The oracle's shots are cached in `~/games/super2sp/dump/oracle/cache/<hash>`,
keyed by the script, its SAVE fixture, SUPERSP.CFG, the exe and oracle.py:
the first run of a script takes minutes (the oracle runs about 40 frames a
second), later runs only rerun s2run (seconds). `S2_ORACLE_DIR` moves the
dump folder, `S2_GAME_DIR` the game.

Scripts (`tools/oracle/scripts/`) use the s2run script language. A line
`#! saves NAME` makes both programs start with the fixture folder
`~/games/super2sp/dump/oracle/fixtures/NAME` as `SAVE\` (otherwise the
game's own SAVE folder is copied); neither program ever writes the real
SAVE folder (s2run runs on a private overlay of the game directory, the
oracle on a scratch copy).

| script | covers |
|---|---|
| `stage1.s2` | boot, intro movies, title, new game, difficulty, prologue, stage-1 dialogue, objectives, first player phase |
| `title.s2` | title menus: settings, load menu, quit prompt |
| `versus.s2` | VS setup: stage list, both army save pickers, the VS battle start |
| `town.s2` | load menu, town square, inn: status list, equipment, items, save/load menus, settings, world map, keeper talk |
| `shop.s2` | shop: three buy lists, a purchase prompt, selling; church: donation, temple, class change prompts |
| `church.s2` | reviving a dead member, a donation, then saving (file compared) |
| `saveload.s2` | saving to slot 2 and loading it back (file compared) |
| `settings.s2` | slider and toggle changes, reopening, saving (file compared) |
| `worldmap.s2` | the inn's world map (keys, pointer, hover, jump), leaving town: march, narration, deployment |
| `after1.s2` | load, march, stage-2 narration, deployment by pointer, placement, battle start (autosave compared) |
| `deploy.s2` | deployment by keyboard: Tab, members, placement with the cursor keys, the facing box, start |
| `ending.s2` | a stage won by re-armed auto-battle with the next stage 0, then the whole ending |
| `classchange.s2` | church class change: the list, cards (one unavailable), the change screen, two changes, saving (file compared) |
| `classchange_hidden.s2` | a hidden class (海盜, unlocked by an equipped item) chosen and the item used up, saving (file compared) |
| `cave.s2` | the hidden-stage flow: stage 59 won by auto-battle, the 進入洞窟 prompt, the cave stage set up (narration, dialogue, objectives; autosaves compared) |
| `cave_return.s2` | the cave stage won, victory dialogue, rewards, back between stages at the town, the world map |
| `ending_r0.s2`, `ending_r1.s2` | each route's last stage from turn 15: the turn-16 withdrawal, the ending variant (flag 76 / 78), epilogues, record, staff page |
| `ending_74.s2`, `ending_80.s2` | the same with the other branch flag: ending variants 74 / 80 |
| `ending_73.s2` … `ending_79.s2` | after 克雷布特里 falls: stage 61 / 131 over at once (flag 4), the victory event's variant 73 / 75 / 77 / 79, the whole ending |
| `classchange_battle.s2` | in-battle class change: Tab, the command row, 轉職, cards, confirm (growth rolls), sprite reload and wipe-in |
| `inn.s2` | the inn's quit prompt, the equipment screen used (unequip, preview, re-equip), 自動裝備, saving (file compared) |
| `combat.s2`, `battle_*.s2` | BATTLE's in-battle scripts (AI, combat, spells) |

Besides the shots, compare.py compares every file the two programs wrote to
`SAVE\` (and SUPERSP.CFG) byte for byte, ignoring only what the original
leaves uninitialised: the copyright block after its NUL (skipped by the
loader) and each unit's runtime pointers (+0xad, +0xb1).

Fixture folders (`~/games/super2sp/dump/oracle/fixtures/`):

| fixture | save |
|---|---|
| `empty` | no saves |
| `after1` | between stages 1 and 2 (world map, no town) |
| `town` | between stages 2 and 3 (a town) |
| `revive` | between stages 4 and 5, two members dead |
| `ending` | the original's own stage-1 turn-1 autosave, next stage set to 0 |
| `classchange` | route 0 between stages after stage 25 (six members may change class) |
| `classchange_hidden` | route 0 after stage 48, 法科爾 holding 銀牙彎刀 (equip slot 5) |
| `cave` | route 0's stage-59 turn-1 autosave, cave-found byte set (campaign block +0x1a) |
| `cave_return` | the cave stage 62's turn-1 autosave from cave.s2, five 魔頭 dead (DEAD set, DEPLOYED cleared), the last at 1 HP |
| `ending_r0`, `ending_r1` | the last stage's (60 / 130) turn-1 autosave with the turn set to 15 |
| `ending_74`, `ending_80` | ending_r0 with flag 42 set, 43 cleared / ending_r1 with 63 set, 64 and 65 cleared |
| `ending_73` … `ending_79` | stage 61 / 131's turn-1 autosave (s2auto `--set-flag 60:15` / `130:15`) with story flag 4 and one branch flag (42 / 43 / 65 / 63) |

`tools/oracle/make_fixtures.sh` creates missing fixtures: s2auto (route 0,
seed 1) plays the first stages with `S2_TEST_SAVE=20`, a test hook in
`flow/town.rs` that writes slot 20 + i on the i-th arrival between stages;
`ending` comes from the oracle itself. `tools/oracle/make_late_fixtures.sh`
makes the late-game ones from one s2auto run per route (`--god`) with
`S2_TEST_SAVE=1 S2_TEST_SAVE_KEEP=DIR`, which also keeps every between-stages
save as DIR/between_<stage>.sav, plus its turn-1 autosaves, each with the one
edit listed above. The auto-battle parts of cave/cave_return/ending_r* were
recorded with `s2auto --oracle-check SAVE --record-only` (its recorder presses
Esc every 240 frames when the system menu is not open and 自動作戰 when it is)
and then edited: the Esc that would answer the cave prompt was cut. Existing fixtures are kept: a newer
port plays the stages differently, and a remade fixture changes the cache
key of every script that uses it (`--force` remakes them anyway). Other
agents add their own fixture folders here (BATTLE's in-battle saves).

## Running the pieces

```sh
# the original alone (shots in OUT/NAME.png)
tools/.venv/bin/python -I tools/oracle/oracle.py --game "$S2_GAME_DIR" --out OUT --script "frames 300; shot a"
# s2run and the original on one script; report + NAME.diff.png per shot
tools/.venv/bin/python -I tools/oracle/compare.py --game "$S2_GAME_DIR" --out OUT --script-file test.s2
```

`compare.py` uses `target/release/s2run` (build it first, or pass
`--s2run BIN`); `--keep-oracle` reruns only s2run, `--cache DIR` caches the
oracle side as check.sh does, `--saves DIR` overrides the fixture, `--log F`
copies the report to F, `--port-only` runs only s2run (on the same private
game dir, so a script's clicks can be worked out cheaply before the oracle
runs). Comment lines in scripts must not contain `;` (both parsers split
steps on it first); compare.py refuses such a script. compare.py holds a
lock (`<oracle dir>.lock`) while it runs: two oracles in one folder (one
cache entry, or one sweep check started twice) would share its scratch SAVE
folder and see each other's saves (a phantom UJ52 made the original back it
up to UJ53).

## What runs

The unpacked executable (`~/games/super2sp/re/WSUPERSP_unpacked.exe`,
base 0x400000, entry 0x47b11f) runs in Unicorn. Each import slot of both
IATs points at a stub address whose code hook answers the call in Python;
DirectDraw objects are fake COM objects whose vtables point at more stubs.
Nothing from the game directory is executed (MSS32.DLL and WDIR32.DLL are
never loaded) and no host API is reached:

| area | emulation |
|---|---|
| files | read-only over the game dir (case-insensitive); `SAVE\` and `SUPERSP.CFG` live in `OUT/scratch`, copied at start, the only writable paths |
| window/input | script steps become WM_KEYDOWN/UP (scan codes, `WM_SYSKEY*` with Alt), WM_CHAR from `TranslateMessage`, coalesced WM_MOUSEMOVE, button messages and WM_*DBLCLK (the class has CS_DBLCLKS); `GetKeyState` is the state of the last retrieved message |
| DirectDraw | exclusive 640x480x16 (the program picks it); RGB565 surfaces in emulated memory; Blt/BltFast/Flip/Lock/GetDC; a flip (or blit to the primary) is a *present* |
| GDI | `SetPixel`/`GetPixel`/`StretchDIBits` on surface DCs (the program's pixel-format probe) |
| Miles | every `AIL_*` succeeds silently; samples report done, no CD drive |
| registry | the AutoRun key opens, its value reads as missing, writes are ignored |
| emulation | one `emu_start` without a timeout, stopped only for a stop reason: a restart can resume on an instruction whose code hook already ran (a `--watch` call was logged twice); `--wall-limit` is checked every 4096 API calls |

Unimplemented imports or COM methods stop the run with their arguments in
`OUT/oracle.log`.

## Time

The oracle follows s2run's virtual clock (`sys.rs`), so the same script
step lands on the same frame:

- a present costs 16 ms and is one script frame;
- an input poll that finds nothing costs 1 ms (s2run's `Sys::input_get`):
  the oracle charges it when the pump's `PeekMessageA` comes back empty
  inside `FUN_00474aac` with no matching queued event, except under
  `FUN_00474c54` (the flush, which s2run does not idle in);
- a busy wait on the clock (`GetLocalTime`, read by `FUN_0046d5d6` inside
  the waits `FUN_0046d626` / `FUN_0046d652` or the polling test
  `FUN_0046d688`) advances 1 ms per read once one call site (its return
  chain) has read it 256 times without a present, message or idle poll: the
  wait ends on the millisecond s2run jumps to, and ordinary reads from
  elsewhere afterwards cost nothing. Reads outside those three never wait
  (a start time, an animation timer, the palette cycle: `FUN_00460fd0` cycles
  it once per map cell and was charged 22 ms before);
- whenever time crosses 16 ms since the last frame, a script frame runs.

The local time starts at 00:00:00.000, so `FUN_0046d5d6` equals s2run's
`ticks_cs`. `time(NULL)` (`FUN_0047ac1b`, the RNG's periodic reseed) is
hooked to return `--time-seed` (default 0 = s2run's `Seeder::fixed(0)`).

`--frame-log F` (both programs; compare.py always writes them, as
`s2run/frames.log` and `<oracle dir>/frames.log`) writes `serial ms` per frame
with ` P` on presents; a `paste` of the two logs shows where the clocks part
and which side presented more.

## Comparing

The original converts its 8-bit frame through `FUN_00475a50`'s table:
`(r & 0xf8) << 8 | (g & 0xf8) << 3 | b >> 3` (green's low bit is always 0).
`compare.py` quantises s2run's palette colours the same way, so a pixel
differs only when the 16-bit values differ. Each shot also gets
`NAME.idx.png` from the oracle: the platform layer's 8-bit shadow frame
(`DAT_004a2ba0 + 0x1030 + 0x618`) with its BGRX palette (`+8`), for index
level comparisons.

Diagnostics (oracle.py): `--dump-units` (both armies' records per shot),
`--peek ADDR:LEN,...` (hex dumps per shot; bit 31 of ADDR dereferences a
pointer), `--watch HEX,...` (log calls with arguments), `--watch-write
HEX,...` (log writes with the EBP call chain), `--log-presents` (each present
with its call chain), `--trace` / `--trace-from FRAME` (every API call and
retrieved message).

Findings so far worth knowing when porting: the image header holds width - 1
and height - 1 (`*(short *)img + 1` is the width, `(… + 1) / 2` is `width /
2`); fades present twice per level and drain input; the repack's disc probe
reads `.\DISK.ID` ("2": disc 2, which selects the second DAT table).
