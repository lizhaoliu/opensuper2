# Game flow (startup → title → campaign → ending)

Source: v1.4 `WSUPERSP.EXE`, Ghidra decompilation with all 33 game-code
jump tables recovered (`tools/find_switches.py` + `ghidra_scripts/ApplySwitches.java`;
merged output is `~/games/super2sp/re/flow/decomp_full.c`, read with `tools/fn.sh <addr>`).
Every claim cites a function. "(guess)" marks inferences I did not confirm.

Message numbers below are indices into the UI string table (BIN entry 5, or 6
when `DAT_004944f4` "mainland wording" is set; loader `FUN_00424122`).
In the decompilation they often appear as byte offsets: `*(DAT_00494768 + 0x35c)` = msg `0x35c/4 = 0xd7`.

## Important globals

| Global | Meaning | Evidence |
|---|---|---|
| `DAT_00494668` | two-player VS mode (1) | title, `FUN_0040d108` |
| `DAT_0049466c` | "between stages" phase: 1 while on world map / in town | `FUN_0040ff7d` |
| `DAT_00494670` | current stage id | `FUN_00423033` |
| `DAT_00494674` | turn number (1-based) | `FUN_0040d108`, `FUN_00423033` |
| `DAT_00494678` | next stage id (0 = campaign over → ending) | `FUN_00423033`, `FUN_0040d108` |
| `DAT_0049467c` | alternate/hidden next stage (cave) | `FUN_0040d108`, `FUN_0040fa1d` |
| `DAT_00494680` | current world-map location | `FUN_0040d108`, `FUN_0040fd3c` |
| `DAT_00494682` | "cave discovered this stage" flag (guess) | `FUN_0040d108` |
| `DAT_00494684` | difficulty 1..5 (default 3) | `FUN_0040de96`, `FUN_00424450` |
| `DAT_00494692` | stage music cue | `FUN_00423033` |
| `DAT_00494694`/`98` | stage header: next / alternate next stage | `FUN_00423033` |
| `DAT_0049469d` | stage type: 1 battle, 2 battle with retreat option, 3 story-only | `FUN_0040d108`, `FUN_0042cc2c` |
| `DAT_00494508..57` | story flags 1..=80 (`DAT_00494507 + n`), all cleared by `FUN_00423ad4`, 1..=40 at every stage end | `FUN_00417a1d` |
| `DAT_00494530` (flag 41) | set by the prologue (stage 140) when 娜塔莉雅 is chosen: 0 → route 0 (白志超, first stage 1), 1 → route 1 (first stage 71) | `FUN_0041a456` |
| `DAT_00497b54` | protagonist route (copied from flag 0x28) | `FUN_0041a456`, `FUN_00415fd2` |
| `DAT_00497b50` | battles fought counter | `FUN_0040d108`, `FUN_00415fd2` |
| `DAT_00497b58` | current disc (0 = disc 1, 1 = disc 2) | `FUN_0046de06`, `FUN_004198d4` |
| `DAT_004944e8` | CD mode (DEVICE drive letter != '0') | `FUN_00423b00` |
| `DAT_004944ec` | movies on (CDDA= second field) | `FUN_00423b00`, `FUN_0041a370` |
| `DAT_004944f4` | mainland (simplified wording) text table | `FUN_00424122` |
| `DAT_0049456c` | game-constant block inside BIN entry 10 (`FUN_0042422b`): `+0x100` prologue stage, `+0xfe`/`+0xff` first stage per route, `+0xfe..` more | `FUN_0041a456` |

Objects: `DAT_004947c8` screen (3 pages, see ui.md), `DAT_00492d38` text
renderer, `DAT_00492bd0` stage script runner, `DAT_00497a68` battle map,
`DAT_0049f9f8` player army, `DAT_004976c8` enemy army.

## Startup — `WinMain` `FUN_004247da`

1. `FUN_004741c0`: create window class "wsupersp" + DirectDraw (`FUN_00474335`, `FUN_00474257`). Port: nothing (frontend).
2. `FUN_00472f50`: no-op.
3. `FUN_00424450` init, in order:
   - toggles `NoDriveTypeAutoRun` in HKCU (restored on exit by `FUN_0042471a`). Port: skip.
   - `FUN_00423b00` read `SUPERSP.CFG` (see "Config" below).
   - `FUN_00419890` audio init (returns error code shown later), `FUN_00407490` CD init (returns 1/2 = drive problems).
   - `FUN_004217b9` palette + archive index; `FUN_00424122` UI strings.
   - `FUN_0046e998(screen, 3)` allocate 3 pages; `FUN_0041de22` fonts; bind text renderer to screen.
   - `FUN_004708e6`, `FUN_0045e8b2` (spell effect index), `FUN_0040fc6f` (world-map image slots = 0), `FUN_00433906` cursor save-under buffer, cursor shape 0xb.
   - clear page; CD errors → `FUN_004321c7(0xe8/0xed)` (fatal "no drive"/"no disc").
   - `FUN_00421f14` load resident UI image table (ui.md "sysimg").
   - audio error → message box msg 0xe9/0xea at (176,200), set palette, click-to-continue.
   - `FUN_0047332b()==0` → msg 0xec 找不到滑鼠 (no mouse). Port: never.
   - clear; message box msg 0xe7 資料讀取中 at (176,200) no wait; set master palette.
   - `FUN_0045e3c7` build remap LUTs (ui.md), `FUN_004217a0` (`FUN_004733b9`, cursor show), `FUN_0046d6b5` (seed RNG, guess), `FUN_00424b1f` map object, `FUN_0042422b` (BIN 10 tables), `FUN_00423737` (DAT 4), `FUN_004239c9` (DAT 1), `FUN_00423a58` (DAT 6), `FUN_0045e4cf` (BIN 2), `FUN_004236c7` (BIN 8), armies `FUN_0043d298` ×2.
   - zero 0x1d bytes from `DAT_00494668` (mode/stage globals), difficulty = 3, clear story flags, `FUN_004242f2` (DAT 3), `FUN_0045e6a5`, copy msgs 0x36..0x45 into `DAT_004975ac` (unit command names), msg 0xc6, `FUN_0042441d` (名聲 `DAT_00497b48` = P[0xf9], 評價 `DAT_00497b4c` = P[0xfa], battle counter 0).
4. `FUN_00430244`: scan save slots (`save\uj00..52.sav`), returns "any save exists".
5. `FUN_0046d442(master, 1, 0)`: fade out (64 steps).
6. `FUN_0041a370`: if movies on: stop music, play ANI 1 then ANI 2 (`FUN_00459566(ani, n, 0x400, 0)`) = opening movies.
7. `code = FUN_0040d7e1()` title. If code ∉ {10, 11} and `DAT_0049466c` → `code = FUN_0040ff7d()`.
8. `FUN_0040d010(code)` top-level loop; then `FUN_0042471a` cleanup + `ExitProcess(0)`.

## Top-level state codes

Returned by screens and by the stage loop `FUN_0040d108`; dispatched by `FUN_0040d010`.

| Code | Meaning | Produced by | `FUN_0040d010` action |
|---|---|---|---|
| 1 | stage won, campaign continues | `FUN_0040d108` | `FUN_0040ff7d()` (go to next stage) |
| 2 | stage lost (game over) | player/enemy phase | reset, back to title |
| 3 | "end turn" chosen in map menu | `FUN_0042cc2c` (consumed inside battle) | — |
| 6 | "auto battle" chosen | `FUN_0042cc2c` (inside battle) | — |
| 7 | a save was loaded (slots 0–49, 51, 52) | `FUN_00430405` mode 0 | if between stages: `FUN_0040ff7d()`, else re-enter stage loop with 7 (resume) |
| 8 | the special save slot 50 was loaded | `FUN_00430405` | same as 7 |
| 9 | save written | `FUN_00430405` mode 1 | — (local) |
| 0xa (10) | enter stage loop for a freshly set up stage | title (new game / 2P), `FUN_0040ff7d` (story-only stage) | `FUN_0040d108(10)` |
| 0xb | quit game | title 離開遊戲, map menu 系統設定→離開遊戲, inn 離開遊戲 | return → exit process |
| 0xd | stage prepared from the world map (deployment done) | `FUN_0040ff7d` | `FUN_0040d108(0xd)` |
| 0xe | enter the cave / hidden stage | `FUN_0040d108` after a win | `FUN_0040fa1d()` then 0xd |
| 0xf | retreat (撤離戰場) | `FUN_0042cc2c` | same as 1 (next stage, no victory rewards) |
| 0x14 | two-player battle finished | `FUN_0040d108` (2P) | reset, back to title |
| 0x15 | (reset to title; producer not found) | — | reset, back to title |
| 0x16 | (→ 10; producer not found) | — | 10 |
| 0x17 | campaign finished (no next stage) | `FUN_0040d108` | `FUN_0041a779()` ending, then return (program exits) |

"Reset" = `FUN_00424946` (free armies, map, stage data).

## Title — `FUN_0040d7e1`

- Images: DAT 19 (`FUN_00420fab(-0x13, pal, 3, 0)`, 640×480 PCX) with its own palette `pal`; DAT 20 = dimmed title used as backdrop while a sub-screen opens.
- Music cue 0x19 (`FUN_004198d4(0x19,-1,1)`), cursor shape 0xc.
- Page 2 = composed background: title image, version text `"V%s"` (small font, color 7, plain) at (250,463), 5 items at x=278, y=286+30·i (big font, fg 0xf, shadow color 3, shadow style).
  Items: msg 0xd4 重新開始, 0xd5 載入進度, 0xd6 雙人對戰, 0x7b 系統設定, 0x83 離開遊戲.
- Selected item redrawn in fg 2. First frame fades in from black with the title palette (`FUN_0046d442(pal,0,-4)` = 16 steps).
- Input: Up/Down (scan 0x48/0x50) move (no wrap, 0..4); Enter/Space (0x1c/0x39) select (waits for key-up); mouse over x∈(265,387), y∈(282,433) selects row `(y-283)/30`; left button on the highlighted row selects (waits for button-up).
- On select: draw DAT 20 opaque, rescan saves; unless (no saves and item = 載入進度) fade out (16 steps), cursor 0xb.
  - 0 重新開始: `FUN_0040de96` difficulty screen; if confirmed: code 10, clear flags, `FUN_0042441d`, 1P mode; after leaving the title loop with 10 (and not 2P) → `FUN_0041a456` new-game start.
  - 1 載入進度: `FUN_004322bc(1)` → load screen; 7/8 exits the title with that code.
  - 2 雙人對戰: `FUN_0040e5db`; if confirmed code 10, clear flags.
  - 3 系統設定: `FUN_0042efcd(0)` settings.
  - 4 離開遊戲: yes/no at (200,403) msg 0x33 離開遊戲? → code 0xb.
  - otherwise fade-out with master palette and redraw the title.
- Exit path: stop music, free images, cursor 0xb, flush input.

## New game

### Difficulty — `FUN_0040de96`

- Loads DAT 7 images 4 and 3 (`FUN_0042267c`); draws image 4 at (173,223) and image 3 at (9,453),(169,453),(329,453),(489,453) on page 0, then copies page 0 → page 1 (background).
- Title msg 0xd7 難度決定 big font color 0x73 at (295,237); button msg 0xdd 出發 color 0x6b at (319,410).
- 5 rows msg 0xd8..0xdc (天真的試探者, 幼嫩的冒險者, 快樂的玩家, 堅毅的勇者, 嗜血的破壞神) big font color 0x34 at x=280, y=267+28·i. Selected: color 0x3c with outline 0x6c, outline style, plus a filled rect color 0x3c (249, 266+28i)-(266, 290+28i) as a bullet.
- Footer (small font, color 0x71): labels msg 0xde..0xe1 (ＡＩ強度, 簡單規則, 敵人強度, 經驗點數) at x=18+160·k, y=457; values for the selected row at x = 110 / 270 / 430 / 590:
  | row | AI | simple rules | enemy | exp |
  |---|---|---|---|---|
  | 0 | 5% | 是 (0xe5) | -20% | 300% |
  | 1 | 25% | 否 (0xe6) | -10% | 200% |
  | 2 | 50% | 否 | 0% | 100% |
  | 3 | 80% | 否 | +10% | 100% |
  | 4 | 100% | 否 | +20% | 100% |
- Default row 2; music 0x19; first draw fades in with master palette (16 steps).
- Keys: Esc (scan 1) cancel; Enter/Space confirm; Up/Down move. Mouse: left click on a row (x 248..437, y 265..404) selects it; left click on 出發 (x 272..412, y 411..438) confirms; right click cancels.
- Confirm → `DAT_00494684 = row+1`, return 1; cancel → 0.

### New-game start — `FUN_0041a456`

1. Hide cursor, clear page, disc check (`FUN_0046de06(0)`; port: no-op), fade out.
2. `FUN_0041a583(constants[0x100])`: the prologue. Loads that stage (`FUN_00423033(stage,10)`), map init, units, draws the map, fades in, plays the stage music, runs the stage's turn-1 scripts (`FUN_00417a1d`, TEXT/SCRIPT doc), stops music, fades out, frees the stage. Stage 140 (選擇) shows the intro dialogue, a choice event (type 5/0, `FUN_0041929b`) and result events; choosing 娜塔莉雅 ends with story flag 41 set.
3. Clear page, message box 資料讀取中 (msg 0xe7), set master palette.
4. Route = flag 41 → `DAT_00497b54`; load first stage `constants[0xfe + route]` with mode 10, map init.
5. Fade out, `FUN_00431ca5(1)` = stage intro narration (ui.md "Narration").
6. Return to `WinMain`, which calls `FUN_0040d010(10)`.

## Stage loop — `FUN_0040d108(code)`

Entered with 10 (new stage), 0xd (from world map), 7/8 (loaded save).
BATTLE owns everything inside the phases; this is the frame:

- 0xb → return 0xb. 7/8 → palette black first.
- Story-only stage (type 3): load units (`FUN_004233c7`), place army, draw map, fade in, music, run turn scripts, then: no next stage → `FUN_0041be4c(0)` (results), else maybe `FUN_0045cfad` (guess: XP/level-up review); stop music, free, fade out → `1` (or `0x17` if no next stage).
- Battle stage:
  1. `FUN_0040d729` adaptive difficulty (enemy level shift from army strength), `FUN_0043abc2(player,1)`, location = stage; battle counter++ unless resuming.
  2. Per turn: (1P) on 0xd: music = stage music and `FUN_00431154`; on 10: `FUN_004233c7` + `FUN_00431154`.
     If turn 1 or resuming: place army, draw map, fade in, music (resume: last cue), `FUN_00432054(320,100,msg 0x103)` banner 記憶體不足無法顯示背景場景, shown once and only after `DAT_0048e944` (initially 1) was cleared by a failed background allocation (line ~22488 of decomp_full.c). Port: never.
  3. Unless resuming (7): run this turn's scripts (`FUN_00417a1d`), then `FUN_0043311d` checks win/lose.
  4. Turn banner `FUN_0043117c` (msg 1 "第 %2d 回", guess), player phase start `FUN_004411cf`, phase banner `FUN_00433453(0)` (CEL2 我方行動 image), on turn 1 `FUN_00424c5f` + `FUN_0042d479` (victory conditions window), `FUN_004447e6`.
  5. Player phase `FUN_004323a3()` (2P: `FUN_0043297f(0)`). Later turns start with `FUN_00433831()`.
  6. Results 0xb/7/8/2/0xf end the stage. `FUN_00440e71` end of phase; 1 → victory script `FUN_00417b74`; `FUN_0045b29d` post-phase check; then enemy-phase scripts `FUN_00417cbd`, `FUN_004411cf(enemy)`, banner `FUN_00433453(1)`, `FUN_004447e6(enemy)`, enemy AI `FUN_0043310e()` (2P: `FUN_0043297f(1)`), same checks; turn++.
  7. End: 2P → message box 藍軍勝利/紅軍勝利 (msg 0x10c/0x10d) at (176,200), fade out, return 0x14. 7/8 → return as is. 2 → `FUN_0041a765`. `FUN_0041be4c(0)` battle report. On a win: `FUN_0046c92c`, `FUN_0045cfad(player)`, `FUN_0045d713(player)`; if a cave was found (`DAT_00494682`) and there is an alternate stage, present page 1 and ask yes/no msg 0x35 進入洞窟 at (160,80) → 0xe; or forced 0xe when alternate == next. Fade out (not for 0xb/0xe), `FUN_0041bc3b` on win, free stage. Return code, or 1 / 0x17.

## Between stages — `FUN_0040ff7d`

1. Free map and armies; read the 30-byte stage record of `DAT_00494678` (BIN 1, `FUN_00422f83`).
2. Record byte 21 == 3 → story-only: `FUN_00415e0d` (turn 1, load stage, map init) → return 10.
3. Otherwise world-map phase (`DAT_0049466c=1`): `FUN_0040fd3c` loads DAT 11 (world map 1280×907 + palette), DAT 12..15 (town, inn, shop, church, 640×480 + palettes), DAT 16/17 (dimmed inn/church backdrops), DAT 7 images 5/6, BIN 4 (location table, 14-byte records: x,y at +8,+10; town flag at byte +10 of the record; centres the map scroll on the location).
4. Location has a town (record flag == 1): loop { `FUN_00422e26(location)` (location text), `FUN_00410763()` town hub } while it returns 7 and the world-map phase is still set. Else: `FUN_00413e9a` (easiest difficulty: gold penalty per unit, guess), `FUN_00415fd2` travel + deployment.
5. Returns 0xd normally, or 7/8 when a save was loaded at the inn.

### Town hub — `FUN_00410763`

Hotspot screen (ui.md) over DAT 12, title msg 0x58 村莊, music 4, cursor 0xc, flags 3 (fade in/out):

| # | Label | Rect | Action |
|---|---|---|---|
| 1 | 0x59 旅店 | (226,211)-(370,296) | inn `FUN_004109b3`; nonzero result leaves the town |
| 2 | 0x5a 商店 | (97,277)-(206,366) | shop hub `FUN_00410886` |
| 3 | 0x5b 教會 | (421,272)-(584,402) | church `FUN_00410c21`, then music 4 again |
| 4 | 0x5c 進入戰場 | (79,419)-(296,479) | cursor 0xb, stop music, `FUN_00415fd2` → 0xd |

Not cancellable.

### Inn — `FUN_004109b3`

Hotspots over DAT 13 (pic 1), title msg 0x59, flags 0xb (fade in/out, fast), cancellable (Esc / right click returns 0).
Before each action: draw DAT 16 (dimmed inn) opaque, cursor 0xb. After: fade out fast, cursor 0xc.

| # | Label | Rect | Action |
|---|---|---|---|
| 1 | 0xc1 大地圖 | (473,60)-(562,176) | world map viewer `FUN_00414b17`, music 4 |
| 2 | 0xc2 老闆 | (310,106)-(380,216) | innkeeper talk `FUN_004149ff(FUN_0046d7ca(2))` (random line, guess) |
| 3 | 0x80 讀取存檔 | (534,184)-(617,303) | `FUN_004322bc(1)` → return 7/8 |
| 4 | 0x81 儲存檔案 | (455,179)-(530,299) | `FUN_0043225f(1)` save screen |
| 5 | 0xc3 整理裝備 | (292,243)-(412,479) | `FUN_00447490(player,0,0,1)` equipment |
| 6 | 0xc0 人物狀態 | (292,206)-(352,240) | `FUN_0042f620(1)` status |
| 7 | 0x50 使用物品 | (398,209)-(420,252) | `FUN_00447490(player,1,0,1)` items |
| 8 | 0x83 離開遊戲 | (48,167)-(120,302) | yes/no (300,200) msg 0x33 → return 0xb |
| 9 | 0x7b 系統設定 | (250,158)-(294,202) | `FUN_0042efcd(2)` |
| 10 | 0xbe 自動裝備 | (121,151)-(266,444) | yes/no (200,200) msg 6 是否確定? → `FUN_0044bcb0(player)` |

### Shop hub — `FUN_00410886`

Hotspots over DAT 14 (pic 2), title msg 0x5a, flags 9, cancellable. Fades with the shop palette before each action.

| # | Label | Rect | Action |
|---|---|---|---|
| 1 | 0x5d 武器部 | (124,265)-(391,476) | `FUN_00410d3a(1)` buy |
| 2 | 0x5e 防具部 | (496,92)-(609,200) | `FUN_00410d3a(2)` |
| 3 | 0x5f 道具部 | (227,105)-(393,245) | `FUN_00410d3a(3)` |
| 4 | 0xc7 收購舊貨 | (404,137)-(494,316) | sell `FUN_00412b55` |

Buy: `FUN_00411068` (list; msg 0x64 greeting, 0x68 not enough money, 0xfb owned count) → `FUN_00411aa2` quantity (msg 0xc8) → `FUN_004120f8` who equips (msgs 0x60, 0xcc, 0xcf, 0xd0). Sell: `FUN_00412caa` (msgs 0x61, 0x62, 0x6e) → `FUN_00413556` quantity (msg 0xc9).

### Church — `FUN_00410c21`

Hotspots over DAT 15 (pic 3), title msg 0x5b, music 0x25, flags 0xb, cancellable; backdrop DAT 17.

| # | Label | Rect | Action |
|---|---|---|---|
| 1 | 0xc4 捐獻箱 | (75,314)-(163,383) | donation `FUN_00413c01` (msgs 0x65, 0x69, 0x6b, 0x6c) |
| 2 | 0xc5 復活泉 | (268,305)-(382,400) | revive `FUN_00413f6d(0)` (msgs 0x66, 0x67, 0x68) |
| 3 | 0xc6 轉職 | (495,204)-(629,317) | class change `FUN_00413f6d(1)` → `FUN_00453986` (msgs 0xd1..0xd3, 0xb2..0xb8) |

### Travel and deployment — `FUN_00415fd2`

1. Hide cursor, stop music, `FUN_004156b1`, `FUN_0040ff73`; load DAT 18 (world map for travel) + palette.
2. Pick the disc for the next stage (route 1 after more than 6 battles, or route 0 at stages 0x3c/0x3d → disc 2). Port: no discs.
3. Page 2: map; CEL2 image 0xcb banner at (176,20) with msg 0xe7 centred; `FUN_00415ec8(10)`; `FUN_00415d1d()` (path drawing, guess); `FUN_00415e41(30)`, `FUN_00415f8a()`, `FUN_00415e41(100)` (marching animation, guess); present; `FUN_00431ca5(2)` narration; fade out.
4. `FUN_0041641c` collects the deployable unit list (stage unit records with `kind` field 0x32, position 0x17/0x19 …), then the deployment screen `FUN_00444bea(player, list, n)` (msgs 0xb9..0xbd: 隊員人數, 出場人數, 自動配置, 觀看戰場, 我軍出陣), `FUN_0041671c`, then `FUN_0042b3dd(0xb)`.

### Cave / hidden stage — `FUN_0040fa1d`

Reset map and armies, turn 1, stage = alternate stage, load + map init, copy page 1 → 0, narration `FUN_00431ca5(0)`, deploy units whose appearance-turn window contains turn 1 (records 0x65 bytes in `DAT_00494730`: +0x33 first turn, +0x34 last turn, +0x31 side), redraw both armies. Then code 0xd.

## Two-player battle — `FUN_0040e5db`

Setup screen: battlefield list ("%2d.%s" names), blue/red army save pickers (msgs 0x10f 藍軍存檔 / 0x110 紅軍存檔 via `FUN_00432331(0/1)` → save screen modes 2/3), msg 0x1e 無 for empty, 確定 (0xce) / 取消 (0x111) buttons. Layout via bevel boxes `FUN_0040f08e(x,y,w,h)` (ui.md): (236,140,150,24), (236,210,150,24), (236,280,150,24), buttons (190,424,82,30) and (372,424,82,30). Start: `FUN_0040f67f(…)`. Returns 1 to start; title then returns 10 with 2P set.
Inside battles 2P changes only the phase functions and the end banner (see stage loop).

## Ending — `FUN_0041a779` (code 0x17)

1. Free map, stop music, `FUN_0041b5dc` (BIN 3: ending texts). Load DAT 40 (`FUN_00420fab(0xf,pal,3,0)` → 0x1a-1+0xf) into page 1 as backdrop.
2. Page text 1 (`FUN_0041f87c` wrapped text at x=0x19 (0x7d mainland), y=60, color 0xff, 28/21/28 layout) → fade in, wait for key (`FUN_0046d81f(0)`), fade out; movie ANI 4 if movies on.
3. Page text 2 at y=120, same; ANI 5.
4. Free armies. Ending rank = first set story flag among 73..=80 (`DAT_00494550..57`, index 0..7) → `DAT_00492d20`.
5. Load DAT 26 (`FUN_00420fab(1,…,3)`), `FUN_0041b489`, `FUN_0041abf4` (results / staff roll, guess), free, `FUN_0041b633`.
6. Movies on → ANI 3.
Then `FUN_0040d010` returns and the program exits.

## Config — `SUPERSP.CFG` (`FUN_00423b00` read, `FUN_00423eb9` write)

```
DEVICE=a,b,s      s[0] = CD drive letter; '0' = no CD mode. The repack uses '.',
                   which makes the "CD path" ".\SUPERSP\..." (relative).
VOLUME=m,c,s      DAT_00494640 MIDI volume, DAT_00494644 CD volume, DAT_00494648 SFX volume (0..100)
CDDA=X,Y          X ON/OFF → DAT_00494650 CD-audio music; Y OFF → DAT_004944ec = 0 (movies off)
SET=a,b,c,d,e,f,g,h
   a DAT_00494638 畫面速度 (screen speed, default 0)
   b DAT_0049463c 訊息速度 (text delay in 1/100 s per char, default 10)
   c DAT_0049464c 確定詢問 (ask confirmations, default 1)
   d DAT_0049464f 自動切換 (default 1)
   e DAT_0049464d 狀況視窗 (default 0)
   f DAT_0049464e 快速切換 (default 0)
   g DAT_00494651 動畫播放 (battle animations, default 1)
   h DAT_00494652 遮掩網點 (dither/mesh shading, default 1)
Version=1.4
```
Argument order verified in the disassembly at 0x423d9f (Ghidra's local ordering is misleading).

## Porting plan (title → new game → first dialogue)

1. Display/input/timing primitives (ui.md §1–3): pages, present, palette + fades, centisecond clock, event queue with the original event codes and scan codes, software cursor.
2. Resources: PCX loader (`FUN_00420fab` id rules), sysimg table (`FUN_00421f14`), DAT 7 / CEL2 on-demand loaders (`FUN_0042267c`, `FUN_00422878`, `FUN_00422897`), UI strings (`FUN_00424122`), remap LUTs (`FUN_0045e3c7`).
3. Text renderer API (`FUN_0041df60`, `FUN_0041f748`) on top of the TEXT sub-task's font decoder.
4. Widgets: message box `FUN_00431f6d`, click-to-continue `FUN_0046d8a8`, wait-key `FUN_0046d81f`, yes/no `FUN_0042c663`.
5. Init `FUN_00424450` (data part only), opening movies (`FUN_0041a370`, needs the GRAPHICS ANI player), title `FUN_0040d7e1`, difficulty `FUN_0040de96`.
6. `FUN_0041a456` → `FUN_0041a583` → stage load (`FUN_00423033`, TABLES/SCRIPT) → map draw (`FUN_004252c9`, GRAPHICS/BATTLE) → `FUN_00417a1d` script → dialogue box `FUN_004188ca` + text typer `FUN_00418e73` (ui.md "Map dialogue").
7. Then narration `FUN_00431ca5`, `FUN_0040d010`/`FUN_0040d108` frame with stub phases.
