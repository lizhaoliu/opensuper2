# QA status (s2auto / oracle checks)

Campaign QA harness: `crates/s2engine/src/qa.rs`, `crates/s2engine/src/bin/s2auto.rs`,
`tools/oracle/sweep.sh`. Runs live in `~/games/super2sp/dump/auto/<run>/`, sweep results in
`~/games/super2sp/dump/oracle/sweep/`.

## Current state

**Done.**
- `tools/oracle/check.sh` (41 scripts): 2135/2135 shots identical, every written save / CFG
  identical.
- Oracle sweep (148 checks: every stage's turn-1 save on both routes, the story branches,
  caves, every 10th between-stages save, VS battlefields 1–10): 146 fully identical (shots,
  draws, saves); 039 and 097 match as far as the original ran (the port is 3 / 20 draws
  ahead at the frame cut). Every check has identical draws and saves.
- Campaign matrix (both routes, `--god` and default, seeds 1–3): no panic, hang or invariant
  failure; all eight endings reached; every stage reached (19 only through forced choices /
  flags, table in History (5)).
- Stalls that are the original's behaviour (checked with `--oracle-check`): stage 6 after a
  route-0 wipe (only lose code is char 1, runs to turn 100), 85 (ledges), 90, cave 134, and
  108 / 128 (unreachable in play; 128 is an unfinished placeholder stage).

**Open.** None in the sweep. (The last forced run's shot diffs — 015, 062 / 073, 012 / 102,
055 — were fixed afterwards: story-stage facing/idle restart, aim frame by move type, army
order in save-unders, page restore after cast effects; the regenerated table is 146/148.)
Not coverable with the shipped data: the slay-item path (FUN_0044b41b) and the teleport
branch of the kind-1 cast scene (see view-port-status.md).

**How to verify.**
- `tools/oracle/check.sh` — the regression gate (all identical, exit status 0).
- `tools/oracle/sweep.sh all` (or `saves`, `collect`, `run [--force]`, `table`; env `JOBS`,
  `FRAMES`, `SWEEP_DIR`): one `s2auto --oracle-check` per collected save; `table.md` in the
  sweep folder.
- One save: `s2auto --game DIR --oracle-check SAVE [--oracle-mode battle|town|vs:K]
  [--frames N]` → `OUT/RUN/oracle-check.txt` (shots, draws, RNG diff, saves).
- Campaign runs: `s2auto --game DIR --route both --max-stages 200 [--god]
  [--random-choices --recover 3] [--seed N]`; coverage helpers (not the original):
  `--caves`, `--weak-enemies`, `--boss-rush`, `--set-flag S:F[=V]`, `--choice S:K`.

## History

Working notes, newest first.

### 2026-10-08 (6): forced sweep after the BATTLE / GRAPHICS / FLOW fixes

`tools/oracle/sweep.sh run --force`, 148 checks: 140 fully identical (shots,
draws, saves); every check now has identical draws and saves (39 and 97 equal
as far as the original got). Remaining shot differences:
- 015 (38/81 from f002340, new since the fixes): after stage 15's turn-1
  deploy event (chars 22 and 74 at frame ~2281) four party units are drawn
  without the original's darkened shading (likely the new scroll_to / unit
  drawing path).
- 062, 073 (one shot each, right after a move): `scroll_exact`'s unit is drawn
  at the wrong place/height for a flyer (062: the demon on the ground instead
  of lifted with its shadow, FUN_00426b87's act-4 branch FUN_00442811 +
  FUN_0042665a) and for a ghost unit (073).
- 012, 102 (one shot each): a death dissolve (FUN_0044f27e) frame differs; 055:
  a cast target's flash tint (FUN_0044f11c) at full red where the original
  shows normal colours — effect frame-step offsets.

### 2026-10-08 (5): the stages no campaign run reached

From the event records (stages.txt / `s2tables stage`): each is reached from
the turn-1 save of the stage that branches to it, with `--choice` /
`--set-flag` (`tools/oracle/sweep.sh extra`, runs `sw2-x<stage>`):

| stage | from | how |
|---|---|---|
| 15 | 12 | victory choice 1 (`--choice 12:1`) |
| 21 | 17 | choice 2 after char 84 falls (`--choice 17:2`; also 19 choice 1) |
| 43 | 41 | flag 16 (= won by the exit, victory set 2) → SetNextStage 43 |
| 44 | 41 | flag 7 (= char 119 falls) → flag 15 → SetNextStage 44 + win |
| 50 | 48 | turn-15 choice 1 (a win before turn 15 also keeps next 50) |
| 51 | 56 | flag 47 (48–50 clear) + choice 1 → flag 22 |
| 55 | 54 | flag 54 (= turn 30 reached) → SetNextStage 55 + win |
| 57 | 56 | flag 49 (47, 48 clear) + choice 1 → flag 20 |
| 64 | 46 | flag 14 (= event cell 21 touched with item 286 in the party) |
| 82 | 79 → 162 | story stage 162 choice 2 |
| 93 | 92 | flag 66 clear (stage 90's victory sets it: next 136) |
| 103, 104 | 100 | victory choice 1; 104 follows 103 |
| 118 | 117 | victory choice 1 |
| 121 | 126 | turn-1 choice 1 |
| 125 | 124 | flag 54 (= turn 30) |
| 132 | 96 | flag 64 set, 65 clear |
| 108 | — | stage 89 sets next 108 when flag 16 is set, but flag 16 is set at turn 1 and the victory choice then overrides next (90 / 92): no play reaches it. Hand-made save: between-stages save after 89 with next stage 108 |
| 128 | — | 未拼場景關: no stage leads to it, no enemies. Hand-made save as for 108 (after 126) |

108 and 128 stall under auto battle (s2auto kills them after 25 turns); their
turn-1 saves come from the run's autosave with the helper's mode 6 undone.

### 2026-10-08 (4): oracle sweep (tools/oracle/sweep.sh)

`tools/oracle/sweep.sh all` (= `saves`, `collect`, `run`, `table`): one
`s2auto --oracle-check` per stage turn-1 save (both routes, the story branches
via `--choice`, the caves via `--caves`), per 10th between-stages save
(`--oracle-mode town`: inn, shop, departure, march, deployment, battle) and per
雙人對戰 battlefield 1–10 (`--oracle-mode vs:K`), 20 in parallel; 4800 frames
(towns 8400), a shot every 60 frames. `run` redoes a check only when its save
changed. Reports and diff images: `~/games/super2sp/dump/oracle/sweep/runs/
<check>/` (`oracle-check.txt`, `compare.txt`, `cmp/`); the table below is
`sweep/table.md`. Since (5) every stage 1–136 has a check (the 19 rarer ones
from `extra`).

Turn-1 saves are copied with the harness' AI-mode changes undone (qa.rs, by
character id). Saves with the helper's go-to-point mode 6 on the protagonist
(first sweep; and route 1 after stage 90, where a stall recovery resumed from
an autosave that already had it and the stage's script changed her class)
diverge for a reason of the original's own: on maps without a class-4 cell
FUN_0042890a returns an uninitialised "found" flag and position, so the
original's AI heads for stack garbage. With clean saves those stages match.

Result (129 checks): 87 fully identical (shots, draws, saves); 36 with
identical draws and saves and one or a few differing shots; 3 save-only
(013, 113, between_026); 3 with draw counts that differ (039 and 097: equal as
far as the original got, the port 3 / 20 draws ahead at frame 4800; 109, see
below). All 10 VS battlefields and 6 of 7 town segments are identical.

Causes (routed):
1. `battle/camera.rs` `scroll_exact` (FUN_00426b87) ignores its unit: the
   original draws `param_3` after every redraw step (FUN_0043ff88, or the
   flyer / sword-rider variants when act is 4) before presenting, so the unit
   that just walked is missing in the 1–2 frames of the camera scroll that
   starts an AI attack. Most single-shot rows (a unit-sized box right after a
   move), e.g. stage 18 frames 3239–3240.
2. `battle/menu.rs` `walk`: event cells are recorded for every move; the
   original's walk animations (FUN_0044c331 / FUN_0044c631 / FUN_0044c963)
   call FUN_0041bd98 only when not VS and `army+0xda == 0` (the player).
   Stage 13: event 8 (InList 21) fires from an enemy move (save byte 0x149).
3. The AI aim panel's spell forecast (FUN_00452178 from FUN_004055a4) is
   computed after the school-EXP gain of the cast (FUN_0045ff9e's learning
   loop): stage 24 frame 2820, 刑天's 無極劍法 shows 89 (prof[15] 1316)
   instead of 88 (1280). Also the small panel-digit rows (31, 38, 122).
4. `between_026`: after 自動配置 and start, two party records have `frame`
   (+0x2c) 4 instead of 0, and one is drawn in another pose (f004080).
5. Timing of spell 109 (a 合擊 technique) cast by the player side: one frame
   takes 28 ms in the original and 16 in the port (stage 34 first sweep,
   109 frame 2357, 113 frame 2717: port presents, original idles); later
   shots and `anim_timer` save bytes differ.
6. Stage 109's extra original draw (#1294, pct from 0x40a2fb twice in a row):
   FUN_0040a2ed calls pct once, so this is the oracle's code hook firing twice
   for one call (an oracle artefact); the rest of the sequence matches after
   that one entry.
7. Stage 16: camera origin and the 我方脫離點 label colour differ from f004440
   (not analysed).

| check | frames | shots identical | draws (original/port) | saves identical | first divergence |
|---|---|---|---|---|---|
| battle_001 | 4800 | 81/81 | 1394/1394 | yes |  |
| battle_002 | 4800 | 81/81 | 1410/1410 | yes |  |
| battle_003 | 4800 | 81/81 | 1371/1371 | yes |  |
| battle_004 | 4800 | 81/81 | 1359/1359 | yes |  |
| battle_005 | 4800 | 81/81 | 1407/1407 | yes |  |
| battle_006 | 4800 | 81/81 | 1322/1322 | yes |  |
| battle_007 | 4800 | 81/81 | 1396/1396 | yes |  |
| battle_008 | 4800 | 81/81 | 1376/1376 | yes |  |
| battle_009 | 4800 | 81/81 | 1355/1355 | yes |  |
| battle_010 | 4800 | 81/81 | 1230/1230 | yes |  |
| battle_011 | 4800 | 81/81 | 1344/1344 | yes |  |
| battle_012 | 4800 | 80/81 | 1495/1495 | yes | f004140 |
| battle_013 | 4800 | 81/81 | 1328/1328 | yes |  |
| battle_014 | 4800 | 81/81 | 1370/1370 | yes |  |
| battle_015 | 4800 | 38/81 | 1240/1240 | yes | f002340 |
| battle_016 | 4800 | 81/81 | 1217/1217 | yes |  |
| battle_017 | 4800 | 81/81 | 1366/1366 | yes |  |
| battle_018 | 4800 | 81/81 | 1373/1373 | yes |  |
| battle_019 | 4800 | 81/81 | 1258/1258 | yes |  |
| battle_020 | 4800 | 81/81 | 1465/1465 | yes |  |
| battle_021 | 4800 | 81/81 | 1459/1459 | yes |  |
| battle_022 | 4800 | 81/81 | 1547/1547 | yes |  |
| battle_023 | 4800 | 81/81 | 1407/1407 | yes |  |
| battle_024 | 4800 | 81/81 | 1475/1475 | yes |  |
| battle_025 | 4800 | 81/81 | 1344/1344 | yes |  |
| battle_026 | 4800 | 81/81 | 1218/1218 | yes |  |
| battle_027 | 4800 | 81/81 | 1276/1276 | yes |  |
| battle_028 | 4800 | 81/81 | 1352/1352 | yes |  |
| battle_029 | 4800 | 81/81 | 1382/1382 | yes |  |
| battle_030 | 4800 | 81/81 | 1404/1404 | yes |  |
| battle_031 | 4800 | 81/81 | 1327/1327 | yes |  |
| battle_032 | 4800 | 81/81 | 1503/1503 | yes |  |
| battle_033 | 4800 | 81/81 | 1658/1658 | yes |  |
| battle_034 | 4800 | 81/81 | 1377/1377 | yes |  |
| battle_035 | 4800 | 81/81 | 1407/1407 | yes |  |
| battle_036 | 4800 | 81/81 | 1535/1535 | yes |  |
| battle_037 | 4800 | 81/81 | 1592/1592 | yes |  |
| battle_038 | 4800 | 81/81 | 1413/1413 | yes |  |
| battle_039 | 4800 | 81/81 | 1334/1337 prefix | yes |  |
| battle_040 | 4800 | 81/81 | 1408/1408 | yes |  |
| battle_041 | 4800 | 81/81 | 1457/1457 | yes |  |
| battle_042 | 4800 | 81/81 | 1435/1435 | yes |  |
| battle_043 | 4800 | 81/81 | 1434/1434 | yes |  |
| battle_044 | 4800 | 81/81 | 1481/1481 | yes |  |
| battle_045 | 4800 | 81/81 | 1380/1380 | yes |  |
| battle_046 | 4800 | 81/81 | 1472/1472 | yes |  |
| battle_047 | 4800 | 81/81 | 1423/1423 | yes |  |
| battle_048 | 4800 | 81/81 | 1419/1419 | yes |  |
| battle_049 | 4800 | 81/81 | 1437/1437 | yes |  |
| battle_050 | 4800 | 81/81 | 1472/1472 | yes |  |
| battle_051 | 4800 | 81/81 | 1260/1260 | yes |  |
| battle_052 | 4800 | 81/81 | 1318/1318 | yes |  |
| battle_053 | 4800 | 81/81 | 1337/1337 | yes |  |
| battle_054 | 4800 | 81/81 | 1429/1429 | yes |  |
| battle_055 | 4800 | 80/81 | 1428/1428 | yes | f004320 |
| battle_056 | 4800 | 81/81 | 1370/1370 | yes |  |
| battle_057 | 4800 | 81/81 | 1304/1304 | yes |  |
| battle_058 | 4800 | 81/81 | 1383/1383 | yes |  |
| battle_059 | 4800 | 81/81 | 1341/1341 | yes |  |
| battle_060 | 4800 | 81/81 | 1298/1298 | yes |  |
| battle_061 | 4800 | 81/81 | 1362/1362 | yes |  |
| battle_062 | 4800 | 80/81 | 1475/1475 | yes | f003480 |
| battle_063 | 4800 | 81/81 | 1302/1302 | yes |  |
| battle_064 | 4800 | 81/81 | 1321/1321 | yes |  |
| battle_065 | 4800 | 81/81 | 1431/1431 | yes |  |
| battle_071 | 4800 | 81/81 | 1411/1411 | yes |  |
| battle_072 | 4800 | 81/81 | 1353/1353 | yes |  |
| battle_073 | 4800 | 80/81 | 1340/1340 | yes | f002820 |
| battle_074 | 4800 | 81/81 | 1397/1397 | yes |  |
| battle_075 | 4800 | 81/81 | 1417/1417 | yes |  |
| battle_076 | 4800 | 81/81 | 1372/1372 | yes |  |
| battle_077 | 4800 | 81/81 | 1293/1293 | yes |  |
| battle_078 | 4800 | 81/81 | 1353/1353 | yes |  |
| battle_079 | 4800 | 81/81 | 1379/1379 | yes |  |
| battle_080 | 4800 | 81/81 | 1534/1534 | yes |  |
| battle_081 | 4800 | 81/81 | 1390/1390 | yes |  |
| battle_082 | 4800 | 81/81 | 1371/1371 | yes |  |
| battle_083 | 4800 | 81/81 | 1362/1362 | yes |  |
| battle_084 | 4800 | 81/81 | 1391/1391 | yes |  |
| battle_085 | 4800 | 81/81 | 1366/1366 | yes |  |
| battle_086 | 4800 | 81/81 | 1326/1326 | yes |  |
| battle_087 | 4800 | 81/81 | 1355/1355 | yes |  |
| battle_088 | 4800 | 81/81 | 1408/1408 | yes |  |
| battle_089 | 4800 | 81/81 | 1353/1353 | yes |  |
| battle_090 | 4800 | 81/81 | 1432/1432 | yes |  |
| battle_091 | 4800 | 81/81 | 1400/1400 | yes |  |
| battle_092 | 4800 | 81/81 | 1362/1362 | yes |  |
| battle_093 | 4800 | 81/81 | 1385/1385 | yes |  |
| battle_094 | 4800 | 81/81 | 1458/1458 | yes |  |
| battle_095 | 4800 | 81/81 | 1486/1486 | yes |  |
| battle_096 | 4800 | 81/81 | 1356/1356 | yes |  |
| battle_097 | 4800 | 81/81 | 1504/1524 prefix | yes |  |
| battle_098 | 4800 | 81/81 | 1324/1324 | yes |  |
| battle_099 | 4800 | 81/81 | 1388/1388 | yes |  |
| battle_100 | 4800 | 81/81 | 1385/1385 | yes |  |
| battle_101 | 4800 | 81/81 | 1500/1500 | yes |  |
| battle_102 | 4800 | 80/81 | 1572/1572 | yes | f004560 |
| battle_103 | 4800 | 81/81 | 1509/1509 | yes |  |
| battle_104 | 4800 | 81/81 | 1372/1372 | yes |  |
| battle_105 | 4800 | 81/81 | 1481/1481 | yes |  |
| battle_106 | 4800 | 81/81 | 1399/1399 | yes |  |
| battle_107 | 4800 | 81/81 | 1570/1570 | yes |  |
| battle_108 | 4800 | 81/81 | 1442/1442 | yes |  |
| battle_109 | 4800 | 81/81 | 1405/1405 | yes |  |
| battle_110 | 4800 | 81/81 | 1464/1464 | yes |  |
| battle_111 | 4800 | 81/81 | 1451/1451 | yes |  |
| battle_112 | 4800 | 81/81 | 1476/1476 | yes |  |
| battle_113 | 4800 | 81/81 | 1465/1465 | yes |  |
| battle_114 | 4800 | 81/81 | 1288/1288 | yes |  |
| battle_115 | 4800 | 81/81 | 1359/1359 | yes |  |
| battle_116 | 4800 | 81/81 | 1449/1449 | yes |  |
| battle_117 | 4800 | 81/81 | 1433/1433 | yes |  |
| battle_118 | 4800 | 81/81 | 1358/1358 | yes |  |
| battle_119 | 4800 | 81/81 | 1449/1449 | yes |  |
| battle_120 | 4800 | 81/81 | 1427/1427 | yes |  |
| battle_121 | 4800 | 81/81 | 1377/1377 | yes |  |
| battle_122 | 4800 | 81/81 | 1381/1381 | yes |  |
| battle_123 | 4800 | 81/81 | 1354/1354 | yes |  |
| battle_124 | 4800 | 81/81 | 1432/1432 | yes |  |
| battle_125 | 4800 | 81/81 | 1414/1414 | yes |  |
| battle_126 | 4800 | 81/81 | 1356/1356 | yes |  |
| battle_127 | 4800 | 81/81 | 1327/1327 | yes |  |
| battle_128 | 4800 | 81/81 | 1236/1236 | yes |  |
| battle_129 | 4800 | 81/81 | 1289/1289 | yes |  |
| battle_130 | 4800 | 81/81 | 1295/1295 | yes |  |
| battle_131 | 4800 | 81/81 | 1343/1343 | yes |  |
| battle_132 | 4800 | 81/81 | 1399/1399 | yes |  |
| battle_133 | 4800 | 81/81 | 1361/1361 | yes |  |
| battle_134 | 4800 | 81/81 | 1285/1285 | yes |  |
| battle_135 | 4800 | 81/81 | 1397/1397 | yes |  |
| battle_136 | 4800 | 81/81 | 1385/1385 | yes |  |
| between_001 | 8400 | 141/141 | 1312/1312 | yes |  |
| between_026 | 8400 | 141/141 | 1381/1381 | yes |  |
| between_054 | 8400 | 141/141 | 1200/1200 | yes |  |
| between_083 | 8400 | 141/141 | 1280/1280 | yes |  |
| between_114 | 8400 | 141/141 | 1253/1253 | yes |  |
| between_141 | 8400 | 141/141 | 1236/1236 | yes |  |
| between_165 | 8400 | 141/141 | 1279/1279 | yes |  |
| vs_01 | 4800 | 81/81 | 1208/1208 | yes |  |
| vs_02 | 4800 | 81/81 | 1207/1207 | yes |  |
| vs_03 | 4800 | 81/81 | 1212/1212 | yes |  |
| vs_04 | 4800 | 81/81 | 1208/1208 | yes |  |
| vs_05 | 4800 | 81/81 | 1208/1208 | yes |  |
| vs_06 | 4800 | 81/81 | 1208/1208 | yes |  |
| vs_07 | 4800 | 81/81 | 1208/1208 | yes |  |
| vs_08 | 4800 | 81/81 | 1215/1215 | yes |  |
| vs_09 | 4800 | 81/81 | 1206/1206 | yes |  |
| vs_10 | 4800 | 81/81 | 1208/1208 | yes |  |

148 checks, 140 fully identical (shots, draws, saves).

### 2026-10-08 (3): stage 61/131 boss, story branches to the other caves

- Stage 61/131 with the protagonist next to 克雷布特里 (`--boss-rush` turn-1
  saves): `--oracle-check` 12000 frames, 2173/2213 draws, all 43 shots and the
  saves identical → original behaviour. No scripted regen or escape (the
  stages' events are turn-1/3 dialogue and the victory flags). The boss
  (ai_mode 2, 1 once hit) finds no attack target at all against the
  god-mode party (best = none even at limit 0), so fight() → idle() →
  approach() backs off a cell per turn and end_action rests: +min(10–20 %,
  300) HP; at ≤ 29 % HP retreat (FUN_0040bf6a). Without the cheat the party
  never reaches him (stays near the start for 99 turns).
- `--choice STAGE:OPTION` (forced choice): 29:1 → 33 → cave 63, 31:2 → 35 →
  36 → cave 65 龍穴, 99:1 → 102 → cave 135: all found and set up, and with
  a forced win return to 34 / 39 / 106, invariants ok.

### 2026-10-08 (2): matrix m5 after the key/flush/WM_CHAR/wait_until/cave fixes

Matrix m5 (both routes, `--random-choices --recover 3 --max-stages 200`,
`--god` and default × seeds 1–3): no panic, no hang, no invariant failure
(239 checks). Endings: god seeds 2 and 3 on both routes (the turn-16
withdrawal of stage 60/130). god seed 1 route 0 lost stage 39 ×4 (villagers /
banner), route 1 stalls at stage 85. Default runs end at stages 3, 11, 12
(route 0) and 80, 89 (route 1); stage 6's wipe runs on to turn 100.
Stalls checked with `--oracle-check` from the stall autosaves: stage 85
(ledges), stage 90 (the protagonist waits beside the exit among the escort NPCs) and cave 134 雙子之塔 (party
cannot reach the tower) play the same in the original (draws identical).

New coverage options (s2auto, not the original): `--caves` (start on the
cave spot, FUN_0044f9a1), `--weak-enemies`, `--boss-rush`,
`--set-flag STAGE:FLAG[=V]`; the driver answers every in-battle yes/no
(進入洞窟 included) with yes; `QA ENDING flag=` reports the ending variant.
- Caves 59→62 瘋人院, 105→134 雙子之塔, 129→133 瘋人院 are found and set up;
  auto battle cannot win them (62/133 lost even with 9999 HP, 134 stalls),
  with `--set-flag 62:4` etc. they complete and return to 60 / 107 / 130 with
  rewards, invariants ok. 33→63, 36→65, 102→135 not reached (branches not taken).
- Endings: all eight variants run to the end: 76/78 (matrix), 74 (60:42),
  80 (130:63, 64/65 cleared), 73/75/77/79 (60:15 or 130:15 → stage 61/131,
  then 61:4 / 131:4 with the branch flags). Stage 61/131 itself is not won by
  auto battle even with `--boss-rush` (克雷布特里 retreats and heals; not
  checked against the oracle).
- Routed: stage 90 turn-33 autosave played 6000 frames — draws and shots
  identical, but the 12 enemy records not yet on the map save sprite_w/h
  (+0xa4/+0xa6) as 0x80/0xa0 in the port, 0/0 in the original.

### 2026-10-08: corrected rules constants (FormulaParams.p +2), oracle checks

- Reviewed BATTLE's `FormulaParams::parse` fix (FUN_0042422b:
  `DAT_0049456c = DAT_004a1f50 + m + 2`): correct; docs/formats-tables.md
  BIN 10 layout updated (2 skipped bytes, `P[272]`). All s2engine tests pass.
  Values now: attack 25 AP (`P[0xee]`), spell 45, rest/item/equip 40, second
  attack +8 (`P[0xf3]`), difficulty 1..5 → −25/−15/−5/+5/+15 %, VIP ×140,
  kill ×4..6, offer limit 12·100 + 5·battles. Reviewed BATTLE's ai.rs edits
  (attack hides the range first; `cast::ai_aim` = FUN_004055a4 presentation
  over `targeting::aim_ai_cast`): fine (the area is built twice when
  presenting, harmless: `target_area` resets it).
- Stage 75 stall: harness artifact. The goal helper's ai_mode 6 (set for exit
  stage 161) stayed on the protagonist into stages 74/75. Fixed in qa.rs: the
  helpers' AI-mode changes are undone after each stage and after a retry
  reload. Stage 75 is now won (17–19 turns).
- Stage 6 stall (route 0): original behaviour. Its only lose code is
  character 1 (route 1's protagonist), so once the route-0 army is wiped out
  nothing fires until turn 100 (FUN_00433161). The oracle plays the same
  wipe from the same save (all 2716 RNG draws identical). s2auto now lets
  such a battle run on to the turn-100 defeat instead of killing it.
- Stage 85 (礦坑上層) stall from turn 11 (units on ledges, no reachable
  targets): original behaviour too (1692 draws identical through turn 20).
- `s2auto --oracle-check SAVE [--frames N]`: records an auto-battle run from
  SAVE as an s2run script (OUT/RUN/check.s2) and runs it through
  tools/oracle/compare.py with RNG watches; report OUT/RUN/oracle-check.txt.
- The QA driver now picks 自動作戰 in the system menu (pixel probe of the menu
  ornament) instead of pressing F5: see the key-table bug below.
- Goal stages (exit / 紅旗): from the third attempt the protagonist starts on
  the goal cell (`EXIT-CHEAT ... class=4|6`); `GOAL-END` reports where the
  go-to-point AI ended (stage 34: stuck at (13,53), 17 from the flag (5,70)).

#### Matrix m4 (both routes, `--random-choices --recover 3`, corrected constants)

| run | route 0 | route 1 |
|---|---|---|
| `--god` seed 1 | lost stage 39 散財童子 ×4 (villagers / banner) | stage 85 stall (original), resume hangs (wait_until) |
| `--god` seed 2 | ending (42 records) | ending (36 records) |
| default seed 1 | lost stage 12 ×4 | lost stage 76 ×4 (fixed since: no exit cells → the cheat uses the 紅旗) |
| default seed 2 | lost stage 11 ×4 | lost stage 89 ×4 |

151 stage-start invariant checks passed, none failed. Without `--god` the
AI-vs-AI campaign is lost within the first 10–20 stages.

#### Routed (fixes verified against the oracle in a scratch build)

Patches: `~/games/super2sp/dump/auto/patches/`. With both, the stage 6/75/77/85
checks have identical RNG draw sequences (count and order) and 55/103, 65/76,
30/39, 49/51 identical shots (before: 15, 8, 18, 14).

1. `battle/menu.rs` `key_code` (FUN_0042b9f8): every scan code is one too
   high (the jump table at 0x42bf85 is indexed by scan − 1, Ghidra's
   `DAT_0042bf84[scan]` already includes that). Original: ESC 0x01 → −1,
   RETURN 0x1c → 0x1c, SPACE 0x39 → −8, Tab 0x0f → −0x15, Q 0x10 → −0x1f,
   W 0x11 → −0x20, M 0x32 → −0x16, `,` 0x33 → −0x13, `.` 0x34 → −0x14,
   F2..F7 0x3c..0x41 → −0x1c..−0x17 (so F4 = 自動作戰, F5 = 回合結束),
   Up/Down 0x48/0x50 → y∓2, Left/Right 0x4b/0x4d → x∓1, Home/PgUp/End/PgDn
   0x47/0x49/0x4f/0x51 → the diagonals (PgDn down-right is missing),
   keypad −/+ 0x4a/0x4e → −0x1d/−0x1e.
2. `battle/menu.rs` `system_menu` (FUN_0042cc2c): after the first draw the
   original calls FUN_004339cc and flushes input (`FUN_00472f9e(0x1f)`).
3. `sys.rs`: keys that translate to characters get a WM_CHAR message
   (TranslateMessage, retrieved before later input) that yields no event, and
   `input_flush` (FUN_00474c54) stops at the first pump that yields no
   matching event; the port drained everything.
4. `game.rs` `wait_until` (FUN_0046d626) waits while `|t0 − now| < n`; the port
   waits until `t0 + n` even when `t0` is ≥ n cs ahead, so a unit
   `anim_timer` from a save made at a later clock freezes `lift_off`
   (stage 85 resume "hang", stack lift_off ← ai::decide).

Remaining small differences: horse idle frame during the turn banner (stage 6
f004800, 18 px), a ground shadow at the right edge (stage 77 f003900, 5 px),
the camera/cursor at stage 75 turn 6 start (f012900..f016800); saves differ
only in uninitialised memory the original writes (header 0x80.., per unit
one byte and 8 pointer bytes).
