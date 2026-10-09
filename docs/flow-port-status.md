# FLOW/UI port status

Owner: FLOW. Title, menus, story flow between battles, towns, save/load, settings,
deployment, ending. Files: `crates/s2engine/src/flow/*.rs`, `ui.rs`, `event/`, `sys.rs`.

## Current state

**Done.** Opening movies, title, difficulty, prologue, dialogue / narration / choices, stage
setup and the battle loop hand-off, world map and town hub, inn (world-map viewer, equipment,
items with their spell-book windows, status list and unit status with the 魔法 / 招式 tabs),
shops, church (donation, revival), class change, march / loading / disc-change pause,
deployment (incl. auto-fill), save / load with autosaves 51–53 (original `SAVE\UJnn.SAV`
format, byte-identical), settings and `SUPERSP.CFG`, 總檢表 and the map overview, two-player
setup, caves, end-of-stage army cleanup and the ending in all eight variants.

**Open.** Nothing known from the oracle in FLOW code. Known deviation: with an empty shop /
temple list the original would index garbage on Enter; the port ignores the key.

**How to verify.**
- `tools/oracle/check.sh`: the FLOW scripts (`title`, `town`, `inn`, `shop`, `church`,
  `saveload`, `settings`, `worldmap`, `after1`, `deploy`, `versus`, `classchange*`, `cave`,
  `cave_return`, `ending`, `ending_73`..`ending_80`, `ending_r0`, `ending_r1`) are all shot-,
  save- and CFG-identical.
- `tools/oracle/sweep.sh` town mode checks every 10th between-stages save (inn, shop,
  departure, march, deployment, battle) against the original.
- Headless jumps: `S2_TEST_SCREEN=ending:<slot>` / `picture:b,c,d` (s2run).
- `cargo test --release -p s2engine` (`save_then_load_round_trips`, army cleanup tests,
  `mouse_flood_does_not_delay_clicks`).

## History

The running checklist as written during the port (oldest first). "In progress" and "Open (routed)" items below are done: BATTLE's system menu opens `settings::system_submenu`, and the in-battle facing picker is shot-identical (`classchange_battle.s2`).

### Done and verified (s2run screenshots in `~/games/super2sp/dump/flow/run*/`)

- Title, difficulty, prologue, stage setup, dialogue/choices (steps 1–7).
- Save/load screen `FUN_00430405` (+ writer `FUN_004312c4`, loader `FUN_00431845`,
  backup `FUN_0043119b`, autosaves 51/52/53); round-trip test `save_then_load_round_trips`.
  Title menu 載入進度 now opens it (run2/s01_load.png → town).
- Settings `FUN_0042deda` / `FUN_0042efcd`, SUPERSP.CFG writer `FUN_00423eb9`; title 系統設定 wired.
- World map load `FUN_0040fd3c`, between stages `FUN_0040ff7d`, town hub `FUN_00410763`,
  inn `FUN_004109b3`, hot-spot menu `FUN_00410060`.
- Shops: buy list `FUN_00411068` + strip `FUN_004118e0`, quantity `FUN_00411aa2`,
  equip-to-whom `FUN_004120f8` (+ `FUN_0045132d`, `FUN_00448e63`), sell
  `FUN_00412b55`/`FUN_00412caa`/`FUN_00413556` (run2/e02, e04, e08).
- Church: donation `FUN_00413c01` + `FUN_004449cd` (run2/f01), revival list
  `FUN_00413f6d(0)` with `FUN_004476d1` panel (run2/f03).
- March + loading + deploy entry `FUN_00415fd2`.
- Class change `FUN_00453986` (+`FUN_00454368`, `FUN_00454594`, `FUN_00454aea`,
  `FUN_00443a4e`) via the CLASSCHANGE API (run2/f06, f08, f09).
- Inn: world-map viewer `FUN_00414b17` (run2/g01, z_hover), equipment `FUN_00447d3f`
  and items `FUN_0044957b` via `party::equipment(game, mode, unit, fade)` (run2/h01, h06),
  status list `FUN_0042f620` + unit status `FUN_0044feb2` (`flow::status`, run2/i00..i03).
- Deployment: `FUN_0041641c`, `FUN_00444bea`, `FUN_00446293`, `FUN_00446095`,
  `FUN_00416ffb`, `FUN_0041671c` (`flow::deploy`, run2/j02, j04, j06, j07).

- Ending `FUN_0041a779` (+`FUN_0041b489`, `FUN_0041ae08`, `FUN_0041afa3`, `FUN_0041abf4`)
  and the scripted picture `FUN_0041b963` (+`FUN_0041b659`, `FUN_0041b711`) = the
  `show_picture` hook (`flow::ending`, run3/e00..e25, p02). `S2_TEST_SCREEN=ending:<slot>`
  / `picture:b,c,d` jump there for headless checks.
- 總檢表 `FUN_0042f033` + map overview `FUN_004301ae` (`status::overview_menu`, `map_overview`).
- Two-player setup `FUN_0040e5db` (`flow::versus`, run3/v00..v06) → `battle::vs::start`.

- VS phases: the stage loop calls `battle::vs::phase(Player)` / `(Enemy)` in VS mode.
- System submenu `FUN_0042d7cc` (`settings::system_submenu`).

- Spell lists of the unit status tabs 魔法 / 招式: `FUN_0045fd41` → `FUN_00464aee`
  (browse mode), `FUN_00464018`, help window `FUN_00464727` (`flow::spellview`, run3/s02, s03).
  `spellview::spell_help(game, idx, browse=false)` is there for BATTLE's TODO(ui) in cast.rs.
- Deployment markers are occluded by the scenery in front (`MapRenderer::occlude`,
  as `FUN_00426417(map, x, y, buf, 0, 1, 0, 0)`).
- Disc-change pause after the march (`FUN_00415fd2` → `FUN_0046de06`).
- Timing pass: every wait / fade / blink / typewriter constant in my screens matches the
  original call sites (`timing_audit.py`, scratch). SET= options in FLOW screens:
  畫面速度 = camera `scroll_to`, world-map viewer and deployment scroll waits; 訊息速度 =
  dialogue typewriter (`@@D` / `@@RD`) and narration; 確定詢問 = `yes_no` auto-yes unless
  forced; 快速切換 = `scroll_to` jumps first; 遮掩網點 = rebake on change. 自動切換 /
  狀況視窗 / 動畫播放 are read only by battle code (BATTLE/VIEW ports read them).

- Coverage audit (docs/coverage.md): end-of-stage army cleanup `FUN_0044414b` and the cave
  placement `FUN_0045cd5b` ported (`flow::armies`, tests); mainland text layout in text.rs;
  record split for stages entered from a save (the load_stage DEPLOYED restore stopgap is gone now that
  BATTLE's `start_stage` places nobody).

- Deployment auto-fill (`FUN_00446095`) fallback fixed: any idle member takes a cell whose own
  character is missing (the original's inner test only looks at the current cell). This was the
  route-0 stage-4 regression. Stage-end effects (`view::motion::stage_end`) called as in
  `FUN_0040d108`; enemy recolour (tint 0xb) in the status/deploy/ending sprites (`FUN_0045292a`).

### In progress / next

1. BATTLE's system menu calls `settings::open(game, 1)` for 系統設定; the original
   opens `FUN_0042d7cc` = `settings::system_submenu` there. BATTLE also has its own
   `overview_picture`; `status::overview_menu` / `map_overview` are the FLOW ports.

### Coordinator items

- Mouse-flood fix in `sys.rs` (`set_mouse` coalescing, test `mouse_flood_does_not_delay_clicks`)
  and `timeBeginPeriod(1)` in `s2app/main.rs`: kept. `sys.rs` only gained the
  `scan::HOME` / `scan::END` constants; `s2app/main.rs` untouched.
- `copy_page` direction: every FLOW call site re-checked against the disassembly
  (`tools/copycalls.py`, pushes before `call 0x46ec6b`). One was reversed and is fixed:
  `stage::cave_stage_setup` (`FUN_0040fa1d` copies page 1 → 0). All others match,
  including the new shop/church screens.
- Resource caching: no per-frame/per-hover disk reads in the menu loops. Images are
  loaded once per screen entry (as the original does); the unit sprite banks for the
  church/class-change screens are read once and cached in `UiState::unit_sprites`.
  `scan_saves` reads the 53 save files once per load/save screen entry.
- Cursor audit: every `draw_present` / `cursor::draw` in flow/*.rs and ui.rs checked
  against FUN_0043399f / FUN_00433ac7 at the same site (`cursor_audit.py`, scratch).
  All pointer-moved branches restore (`true`) and full redraws do not; one fix:
  deploy placement now redraws the whole frame on a pointer move (FUN_00416c1f(2)).
- `.data` initial values: every global cited in my files checked in the exe image.
  Nonzero ones (dialogue box origin 6/0x7d, cursor -1/-1, palette cycle table,
  hot-spot rects) match; the config reader also sets the music cue `DAT_00494653 = 1`
  (now done in `game_init`), and the viewer's hovered place `DAT_00492b58` persists
  between visits (now kept in `World`).
- Step B: stage loop uses BATTLE's map/armies and the VIEW API; BATTLE has wired
  `player_phase` / `enemy_phase` / rewards into `flow/stage.rs` (verified, not edited by me since).

### Oracle verification (`docs/oracle.md`)

`tools/oracle/check.sh` runs every script in `tools/oracle/scripts/` against the
original (screens pixel by pixel, written saves/CFG byte by byte). Mine: stage1,
title, versus, town, shop, church, saveload, settings, worldmap, after1, deploy,
ending, classchange, classchange_hidden, cave, cave_return, ending_r0, ending_r1.
Fixed in my files from its findings:

- fades present twice per level and drain input each level (FUN_00474fc3);
- `sleep_cs` ends on a centisecond boundary like FUN_0046d652;
- the FLC player skips empty frames (no present), as FUN_00459566 does;
- image header = size - 1: `(*(short *)img + 1) / 2` is `w / 2`, `+ 1` alone
  is `w`/`h` (status, settings, deploy, party, equipment, items, temple);
- `draw_unit` = FUN_0045292a (sprite at `x - w/2, y - h`; decoded frame
  between stages, the type's first frame in battle);
- start-up disc probe FUN_00407490 (`<drive>\DISK.ID`, "2" → disc 2) and New
  Game keeps it in CD mode (the original could not load our saves before);
- loading reports progress at the original's 11–12 steps;
- settings: 易/難 style 1 everywhere; the difficulty number outside the inn;
- deployment: no row focused at first (`local_6c = 0`); placement frame
  FUN_00416c1f (pointer-following cursor, member hidden off free cells,
  0x16 cs animation); every cell marked, stamped into the baked surface;
- the world-map viewer has no extra idle per empty poll (FUN_00414b17);
- save fidelity: sprite slots `+0xa0` and portrait slots `+0xa2` with the
  stage's sprite-type list (`battle/slots.rs`: FUN_0043f42c / FUN_0043e83e /
  FUN_0043eb5f / FUN_0043eef6 / FUN_0041671c's eviction / FUN_0044306f /
  FUN_0043fa24), wired into stage loads, unit creation, deployment, loading;
- battle keys (`battle/menu.rs` key_code, FUN_0042b9f8): every arm was one
  scan code high (Up did Home's move, Left did nothing); now the original's
  table, plus the missing PgDn arm;
- temple redraw: FUN_0043399f then FUN_0046eb48, two presents (the first
  palette-cycle present now lands on the original's frame);
- class-change list FUN_0043b1e5: eligible and not dead, in reserve, an NPC or
  a summon (the port offered dead and reserve members);
- cave discovery (`battle/menu.rs` cave_check, FUN_0044f9a1): a 0xff spot
  leads to header +0x10 (`secret_next[0]`), 0xfe to +0x11; the port read one
  byte too far, so no cave could ever be found;
- dialogue speakers (FUN_0043a860 mode 0): a member in reserve and not on the
  map does not speak (the port showed dead reserve members' lines);
- stage records that reposition a member already in the party
  (`battle/deploy.rs` filter_existing, FUN_0043dc8b) write the facing byte
  only: the view no longer starts that facing's action, so the frame byte
  (+0x2c) and the pose stay (found by TABLES' sweep, between_026).

Status (full check.sh): stage1 55/55, title 9/9, versus 11/11, town 16/16,
shop 16/16, church 13/13, saveload 6/6, settings 11/11, worldmap 14/14,
after1 15/15, deploy 12/12, ending 34/34, classchange 16/16,
classchange_hidden 11/11, classchange_battle 13/13, inn 17/17, cave 10/10,
cave_return 34/34, and all eight ending variants: ending_73 43/43,
ending_74 53/53, ending_75 43/43, ending_r0 (76) 54/54, ending_77 38/38,
ending_r1 (78) 49/49, ending_79 38/38, ending_80 49/49; every written save
and CFG identical. Every inn hotspot, church service and shop list now has a
script. Open (routed): the facing picker after an in-battle command
(FUN_0044d7c1) draws its arrows over the units in front; the original lets
them cover the arrows (repro: dump/oracle/repro/classchange_battle_facing.s2).

### Known issues / deviations

- Shop/temple guards: with an empty list the original would index garbage on Enter;
  the port ignores the key.
- The headless runs wrote `SAVE\UJ51.SAV`/`UJ53.SAV` (autosaves, as the original does);
  the test save `UJ02.SAV` made for the shop/church screenshots was deleted.
