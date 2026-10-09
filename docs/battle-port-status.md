# BATTLE port status

Owner: BATTLE. Files: `crates/s2engine/src/battle/*.rs` except `view*.rs` (GRAPHICS, see
`view-port-status.md`).

## Current state

**Done.** The whole battle program is ported from the original's routines (each cites its
`FUN_xxxxxxxx`): stage start and deployment, summons, movement and terrain (teleport tiles,
damage / refill cells), attacks with counters and deaths, the rules and RNG (`rules.rs`,
`rng.rs`; FUN_0042422b's constants), spells of every kind incl. combos, chains, summons and
teleports, items and the item quake, class change, story hooks and scripted moves, turn
upkeep, objectives, victory / defeat and rewards (EXP, level-ups, drops), the original enemy
AI (`ai.rs`, `targeting.rs`), the player command menu with the original key table, the system
menu, cave discovery and two-player battles (`vs.rs`). Save fidelity: sprite / portrait slots
(`slots.rs`) make every save the oracle writes byte-identical.

Optional, not part of the original (`Game::options`, opensuper2.ini, s2auto flags):
- `ai=enhanced` (`--enemy-ai enhanced`): the original AI's decisions (`ai::fight`) with a
  varied pick (`ai::vary_pick`): same-class spell/technique/attack plans within 80 % of the
  original's best, weighted (score/best)², support buffs as a low-weight stand-in when AP is
  left to attack; spells always weighed from every casting cell; the enemy army buffs its own
  side (`targeting::score_support`, `Battle::ai_varied`). VIP-kill plans are never replaced.
- `ai=tactical` (`--enemy-ai tactical`): the threat-map AI of `ai_plus.rs` (formerly `enhanced`).
- `enemy_boost=N` (`--enemy-boost N`): N % added to the original's per-stage enemy army bonus
  (`turn::adapt_difficulty`), applied once per stage (not again on reloads), never in VS.
- Eval (`tools/ai_eval.sh` + `tools/ai_variety.py`, 17 stages × 5 seeds, player on 自動作戰):
  player wins original 44 %, enhanced+0 60 %, enhanced+20 17 %, enhanced+40 3 %; enemy casts
  use 27 distinct spells vs 17 and 1.6 vs 1.2 distinct spells per casting unit.
- Off (`ai=original`, boost 0) everything is the original: check.sh 2135/2135.

**Open.**
- Sweep residues in battle code (docs/qa-status.md): `camera::scroll_exact`'s unit drawing for
  a flyer / ghost right after a move (sweep checks 062, 073; FUN_00426b87's act-4 branch) and
  the shading of party units after stage 15's turn-1 deploy event (015).
- Data-unreachable corners, deliberately left as they are (no spell uses them): kind 2 without
  a combo pays MP from whatever unit the last combo probe left at +0x0c (the port lets nobody
  pay); a cost-0 kind-2 chain stops after one hit (the original would loop).

**How to verify.**
- `cargo test --release -p s2engine` (battle tests read the game data from `S2_GAME_DIR`,
  default `~/games/super2sp/extracted/app`; skipped without it).
- `tools/oracle/check.sh`: the battle scripts (`stage1`, `combat`, `battle_r0_*`, `battle_r1_*`,
  `battle_vs`, `spells_r0_005`, `view_*`, `cave*`, `ending*`) are all shot- and save-identical.
- Traces: `S2_BATTLE_TRACE=1` (selections, moves, attacks, casts, deaths, phases),
  `S2_RNG_TRACE=1|2` (every draw; the oracle's `--watch 46d776,46d7ca`), `S2_PRESENT_TRACE=1`.
  Test-only: `S2_TEST_UNIT_HP=char:hp` caps a character's HP at each phase start.
- Enhanced AI: `tools/ai_eval.sh` (original vs enhanced over 17 natural turn-1 saves).

## History

The running checklist as written during the port (oldest first).

### Done
- Runtime structs, deployment, summons (FUN_0043e5dd); movement; combat via `battle::rules`; EXP / level-up log.
- Attack + counters + death (actions.rs) with BattleTalk / UnitDown polls; forecast, reward, level-up panels.
  The attack frame loop of FUN_00436e46 / FUN_00437b9c: hit shake from FUN_0043a1b0(frame + 2) = 0 to the last
  frame (view::frame_ahead / view::step_frame, two small wrappers added to view.rs for this).
- turn.rs: upkeep, phase reset, terrain (incl. 0x23 teleport FUN_0042b230/FUN_00458334), banner, wake-ups,
  end turn, rest, AI hookup (`ai::decide` in `ai_act`, enemy phase = npc_phase), victory rewards, survivor EXP,
  objectives screen (FUN_0042d479), adaptive difficulty (FUN_0040d729).
- menu.rs: player phase, input codes, W/E shortcuts, command menu, attack, move, rest (flyers land: FUN_0044cb5f),
  view range (FUN_0044fb84 with the range frame FUN_00470dad), items/equipment/class change/status (FLOW screens),
  system menu (FUN_0042cc2c: 總檢表 = FLOW `status::overview_menu`, 系統設定 = FLOW `settings::system_submenu`),
  map overview key ',' = FLOW `status::map_overview` (also in vs.rs).
- Map attributes: `Map::object_class` is already `attr >> 8` (FUN_004256db callers shift once); fixed the
  double shift in hooks.rs on_exit_tile ×2 / reveal_special_tile, turn.rs phase_terrain, menu.rs after_action
  (exit win codes 0x1f/0x20, tile damage/refill/cleanse, story flags 7/5). Test: map 16's exit (1, 19).
- Keyboard cursor moves (FUN_0042bfd6 → FUN_00426056): −5 when the cell leaves the window, the previous
  cell (`cursor_prev`, DAT_00497580/84) back when the cursor's top is above row 1; −5 runs FUN_00432ecf
  (`menu::follow_cursor`, step scrolling with the mode's redraw) in the player / VS phase, move pick,
  view range, aim and teleport pick. While aiming, after a scroll, FUN_0042c38a (`pull_cursor_into_view`).
- `start_stage` (FUN_00423033's record part) places nobody; `start_stage_placed` (FUN_0040f67f + FUN_00443482)
  is the VS / save-load start.
- Pointer mode `DAT_0048ea98` (−1 in .data) = `Battle::map_click_mode`, set by `menu::set_click_mode` (FUN_0042b3dd)
  where the original does (after an attack, unit status, status list); cursor cell starts at (−1, −1).
- Cursor audit: every FUN_0043399f(-1, 1) site after a pointer move (player phase, rest box, system menu,
  spell menu, aim loop, VS phase) restores; FUN_00433a1f before pressed-button / banner redraws.
- cast.rs: 魔法 / 招式 (spell menu FUN_00464aee/FUN_00464018 with the help window = FLOW `spellview::spell_help`,
  aim FUN_00401634/FUN_004041d7, teleport destination FUN_004052e3 (exact: cursor shapes 9/10/0xf, msg 0x4e+/0x51+)).
  `cast_effects` = FUN_00465f44 (GRAPHICS `view::cast::cast_animation`) with the per-kind sequence FUN_00466f65
  (`cast::sequence`): kinds 2 (chain over visible foes, MP per hit), 3 (cells after the first), 6 (four cells at
  a time after the first) apply FUN_00460c85 cell by cell inside the animation; EXP capped at 0x3c; the
  proficiency gain stays the previous cast's (`Battle::spell_prof_gain`, +0x24). Then FUN_0045ff9e
  (`cast_spell` / `cast_spell_applied`) and the rewards in the original order (partner window + level-ups,
  then the caster's with drops and the learned spell's name; silent for the enemy). The AI casts the same way.
- spells.rs records what FUN_00460c85 / FUN_004636cd / the status spells show (`Cast::shows`: hit flash +
  sound 0x76, the HP popup, deaths, redraws); `cast::play_shows` plays them (per cell inside sequences),
  `spell_hp_popup` = FUN_004636cd's display (HP counted in 1,2,4.. chunks under the forecast panel).
  FUN_004636cd's heal clamp only picks the sign (no heal at full HP; a larger heal overshoots, as the original).
- Items: FUN_0044a619's popup (FUN_00439cc0 with −shown, shown positive); slay-item failure msg 0xe4 via
  `EffectHooks::message`.
- event/battle_hooks.rs forwards to battle/hooks.rs; scripted moves use `view::motion::animate_move` (fly/teleport).
- flow/stage.rs battle loop (FUN_0040d108); story-stage rewards; flow/newgame.rs army clears; flow/party.rs in-battle item use.
- Tests (82 in s2engine): + kind-6 sequence (first cell spared, EXP cap) and cast presentation frames
  (`S2_BATTLE_DUMP=dir` writes PNGs).
- s2run: stage 1 victory / defeat (screenshots in ~/games/super2sp/dump/battle/). s2auto --no-workarounds
  (~/games/super2sp/dump/battle/auto/): b3 both routes, AI alone: route 0 wins 1–6 (incl. 154), loses 7 ×4
  (志超 falls); route 1 wins 71, loses 72 ×4 (char 4 falls). b4 route 0 --god: 20 stages won (to 25). No crash,
  hang or stall.

- Raw Big5 names (`name_raw`) and the action's animation set (`anim_set`) come from `s2data::tables`
  (ClassDef / ActionDef / ItemDef); the separate KINDSP/ITEMSP re-parse is gone.
- FUN_0043abc2's item ability 1: `recalc_army_flags` returns the 名聲 cost, `turn::pay_fame` takes it from
  `Campaign::fame` (DAT_00497b48, already in FLOW's 0x1c save block) while > 0 (flow/stage.rs call site).
- FUN_0044bfb2: event ops 27/28 and items 0x41/0x42 learn spell ranges with the "<name> <msg 3> <spell>"
  windows (`hooks::show_learned`; items through `EffectHooks::learned`).
- Scripted moves: FUN_00458e13 (flight, 8 cs a cell) and FUN_00458ff3 (teleport) ported in hooks.rs; mode 1
  keeps the camera still as the original.
- Teleport spells: the moved unit is the one on `target_cell` (FUN_0046cf00) for every caller (player, item, AI).

- Oracle (docs/oracle.md): `tools/oracle/scripts/stage1.s2` is pixel-identical shot for shot. Fixed for it:
  image headers hold width − 1 (window centring in turn.rs / menu.rs; FUN_0044d7c1 uses the raw header);
  FUN_0042d479 erases the pointer first; the pump `actions::pump` (FUN_004198a4 + FUN_0046d2da, palette
  cycling) at every original call site in battle code (loops of the player / VS phase, command loop, attack,
  move pick, rest box, view range, system menu, follow-cursor, banner, auto / NPC phases, end turn, AI act,
  army withdrawal, camera scroll steps, attack frames and results, popups, spell menu, aim / confirm /
  teleport pick, spell popup, sequences 3/6; the effect passes record `Show::Pump` per cell); SetCursorPos
  posts a mouse move (sys.rs `warp_mouse`, as Windows and the oracle).
- `tools/oracle/scripts/combat.s2`: three turns of 自動作戰 from stage 1 (both sides played by the AI):
  all 99 shots pixel-identical to the original (moves, RNG draws, attack forecasts, damage, kills, spell
  casts, EXP windows, level-ups, AI choices). Fixed for it: s2data `FormulaParams.p` started 2 bytes early
  (FUN_0042422b: `DAT_0049456c = DAT_004a1f50 + m + 2`) — every rules constant was read from the wrong
  offset; the AI attack hides the move range first (only the range frames draw it); the AI's spell aim is
  presented as FUN_004055a4 (`cast::ai_aim`: camera, range, cursor, panel, 2 cs and 5 cs prompts).
- Diagnostics: `S2_RNG_TRACE=1` prints every `pct` / `rnd` draw (`=2` with the battle callers); the oracle's
  `--watch 46d776,46d7ca` logs the original's. `S2_PRESENT_TRACE=1` logs every present (with the engine
  callers) and palette rotation (oracle: `--watch 46eb48,47311a`).
- s2data `passage`: object terrain effects come from class 1's row (FUN_00427e1f has no class offset there).
- Item quake (FUN_0044b515 / FUN_0044b594): shake, per unit the pump, camera, popup and death
  (`EffectHooks::quake`); skipped between stages as the original.
- Story flags in battle code audited against 0x494508 + k − 1: FUN_0044fae0 writes flags 8 / 6
  (fixed); the magic / technique seals are flags 9 / 10 (`battle::sync`, now also at the command loop).

### Oracle coverage (mid/late game)
Fixtures: `tools/oracle/make_battle_fixtures.sh` (s2auto `--god` turn-1 autosaves → ~/games/super2sp/dump/oracle/fixtures).
Scripts (tools/oracle/scripts, run by check.sh): load slot 1, then 2–3 turns of 自動作戰 with shots every 100 frames.

| script | shots identical (current oracle; all saves identical, no clock splits) |
|---|---|
| battle_r0_011 / _030 / _050 / _060 | 58/58, 70/70, 67/67, 102/102 |
| battle_r1_077 / _100 / _118 / _136 | 55/55, 59/59, 155/155, 61/61 |
| battle_vs | 131/131 |
| stage1 / combat / ending | 55/55, 101/101, 34/34 |
| spells_r0_005 / GRAPHICS' 8-turn stage-5 run | 130/134, 254/258 (t4_01–04: 164 px surface residue at turn 4 start) |

No AI / rules divergence: moves, targets, spells, damage, kills and EXP match shot for shot everywhere.
Fixed in this pass:
- `cast::aim_panel` (FUN_00401d75) runs FUN_0042c121's pointer tracking (`view::cursor_track`) before the
  cursor: while the AI aims, a pointer moved since the last call (the 自動作戰 yes/no warps it) moves the
  cursor, so later status bars and the panel side (DAT_0048ea9c) match (r0_011 t1_08+).
- The spell HP popup's panel is placed by the target (FUN_00452178(.., 0) → FUN_0045210f), not the cursor.
- `view::panel::to_big5` prefers the standard Big5 rows over the HKSCS rows (者 has both); class names and
  spell names come from the stored bytes (`ClassDef::name_raw`, `cast::spell_name_raw`).
Routed: GRAPHICS — the remaining diffs above. Saves (unit +0xa0 / +0xa2) — fixed by FLOW (`battle/slots.rs`).
The oracle's 1–2 ms spin-timing splits (clock reads after a busy wait advancing time) are fixed in oracle.py.
Second pass (after the oracle's spin fix; no clock splits left):
- Counters (FUN_00437b9c) and attacks (FUN_00436e46): during the hit flash, popup and death the other
  struck units and the counter-attacker get their state back (the original only lifts them off the
  surface), so a unit in front covers a flashing / dying one (spells_r0_005 t4_30).
- Swing and hit-loop frames draw the combatants with `draw_combatants` (window, then FUN_0043ff88 per unit
  back to front, the original's draw order breaking same-row ties) instead of the whole map.
- Spell kills (`cast::play_shows`): the victims stay on the map until their `Show::Die` (the rule pass had
  marked them dead, so they vanished before their HP popup; ending.s2 a02 now 34/34).
- Combo casts (FUN_0045ff9e): the partner's EXP / level-up windows are taken off (FUN_0046ec6b(1, 0))
  before the caster's (8-turn stage-5 run t6_04, t7_06).
- Keys sent while the enemy phase runs (stage 11, ESC / 3×DOWN / RETURN / RETURN mid enemy phase): both
  programs drop them, 38/38 identical.
- Verified: RETURN on 志超 after loading the stage-5 autosave selects him (repro script, 11/11 identical; the
  key table fix); the turn-5 camera of the 5-turn spells run matches (t5_01..30 identical).
- Saves at 0x32a (unit +0xa0) are identical since FLOW's `battle/slots.rs`.
- Routed to GRAPHICS: spells_r0_005 t4_28 (AI forecast frame: a unit in range drawn over the selected
  attacker in front; `cut_by_units` skips ACT_SELECTED with p5 = 0), battle_r0_030 t3_06 (swing frame);
  8-turn stage-5 run (GRAPHICS' sp/turns8b.s2, 248/258): t6_06 / t6_18 the combo caster left as a hole in
  the surface by FUN_00441c0b in FUN_0045ff9e until the next bake, t7_16 a spell beam over a camel's head.
Third pass (with GRAPHICS' `view_surface.rs`):
- Spell casts: the partner's / caster's level-ups (FUN_00453725) run after their reward windows, not in the
  rule pass, so the HP popups show the pre-level stats (battle_r1_118 t2_30: 邪魔使者 lv 55 599/599).
- Swing and hit-loop frames (`draw_combatants`): page 2 (the window once the combatants are lifted) then the
  units one by one in the original order — targets then attacker (counter swing: struck units then the
  counter-attacker; counter hits: counter-attacker first) — each cut by the others in the state the original
  gives them there (targets restored after the rolls; counter swing all struck at 0; counter hits: the
  others restored, attacker 0). Fixed 030 t3_06, 050 t2_01, 100 t3_08, 118 t2_19, 136 t2_06.
- Explicit surface calls: spell teleports (FUN_00460fd0: caster put in for the move, erased after), combo
  rewards (FUN_0045ff9e: caster put in, partner put in after its window, caster erased), spell deaths draw
  the caster on the page before FUN_0044d14b. Player menu moves and event hooks still use `view::bake`
  (reconcile); spells_r0_005 t4_01–04 (residue near the combo partner at turn 4 start) is open.
Cave scripts (FLOW's cave.s2 / cave_return.s2):
- A dead unit loaded from a save gets FUN_00443c84 (FUN_0044306f): facing byte 0, slot 0, unknown kinds as
  type 1, and keeps its frame byte — `slots_loaded` sets them, `stage::after_load` skips dead units
  (FUN_004433de), and `view::sync_sprites` starts no action for units off the map. cave_return 34/34, saves
  identical.
- Dead enemy extras (npc 1, no character) loaded from a save get no frame in FUN_0044306f: their sprite
  sizes (+0xa4 / +0xa6) stay 0 (`slots_loaded` clears them, `view::store` keeps a dead unit's cleared sizes).
  TABLES' stage-90 turn-33 check: saves identical. Test `loaded_dead_units_sprite_fields`.
- cave.s2 frame 3239 (+0xa8 anim timers 2 ahead in its saves) is the oracle's spin model: FUN_00460fd0's
  per-cell loop calls the palette cycle (FUN_0046d2da, one GetLocalTime each) over a large area with no
  present; past SPIN_FREE (256) reads each further read costs 1 ms (22 ms here) though nothing waits.
Sweep items (docs/qa-status.md, 2026-10-08 (4)):
- `camera::scroll_to` / `scroll_exact` (FUN_00426905 / FUN_00426b87) draw their unit after each redraw step
  (lifted over its shadow for a flyer that has just moved, FUN_00457345 / FUN_004573cb for the mounts).
- `menu::walk` records touched event cells only outside VS and for the player army; move type 2 (FUN_0044c963)
  only the last cell.
- Spell popups show the caster's / partner's proficiency from before FUN_0045ff9e's tail raised it.
- `hooks::reveal_special_tile` (FUN_00428774) keeps the last cell of the class, not the first (stage 16's
  exit label and camera).
Re-run: battle_013 / 016 / 018 / 024 / 031 / 038 / 122 fully identical; spells_r0_005 134/134.
Enhanced enemy AI (not the original; `battle/ai_plus.rs`):
- Off by default (`Game::options.enhanced_ai`, `s2engine::run_with_options`; the frontend maps opensuper2.ini
  `ai=enhanced`; s2auto `--enemy-ai enhanced`). When on, `ai::fight` (FUN_0040aadc) of enemy-army units outside VS
  and not berserk goes to `ai_plus::fight`; the modes, wake-ups and story moves stay the original's (go-to-point
  units keep `ai::approach` when they have nothing to do). Off: check.sh 1379/1379.
- Per action over the unit's range: attacks (expected damage relative to the target's HP, kills, target priority —
  the stage's lose-condition characters and escorts, the protagonist, healers / casters / summoners, the units
  that hit hardest — minus the expected counter, a second attack's AP valued), spells (the original's aim per spell,
  then the area's value: damage, heals by need, debuffs, own-side buffs the original never casts), the cell ranked
  by the threat map (two strongest opponents' expected hits there next turn, after the terrain's defence; leaders
  keep safer). No action: hurt units with a healer around step out of the threat; units alone in front wait at
  the threat's edge for the group (up to turn 8); the rest advance on the best target over the safest cells.
  The evaluation draws no RNG (snapshot / restore).
- Evaluation `tools/ai_eval.sh` (17 stages, natural turn-1 saves, player on 自動作戰). 10 seeds: player wins
  44% (original) → 35% (enhanced; the 29% of the first 5-seed run was luck, ±4% is noise at 170 runs), turns
  10.8 → 10.1, no unfinished battles; think time median ~10 ms, p99 ~100 ms per unit (release).
- Second pass: items (army consumables — no enemy record in the campaign carries any: their item lists are
  equipment, healing items only appear as drops on stages 28 / 98), the 2-ply reply ranking (threat without
  the likely-killed units), an objective term (lose-condition progress). All neutral against 自動作戰 (35%);
  tried and dropped as worse: healers / casters kept back (−5..−7 points), a higher bar for spells, holding
  every unit early, hit-and-run, converging on one focal target.
- Tests: `ai_plus::tests` (threat map coverage / no RNG, priority, plans put the unit back).
Next: stages 6 / 75 are TABLES'.

### Next / known gaps
- s2auto (AI alone) after the constants fix: stages 6 (沙盜營地) and 75 (驅離守衛) stall (25+ turns without an
  attack, mounted units side by side); not yet checked against the original (an oracle run of the stage).
- GRAPHICS owns the remaining TODO(view) / TODO(ui) markers in battle/* (incl. the spell aim overlay
  FUN_00401bfa / FUN_00403e52, approximated with the move-range overlay).
- FLOW: the between-stage item use (flow/party.rs) passes `MarkDead`, so items 0x41/0x42 show no windows there;
  passing `BattleEffects` and calling `battle::hooks::finish_effects` (or `show_learned`) would.
- Data-unreachable corners (no spell uses them): kind 2 without a combo pays MP on +0x0c, whatever unit the
  last combo probe (FUN_0046570c) left there — the port lets nobody pay; a cost-0 kind-2 chain is cut after
  one hit (the original would loop). Kind 6 pads its last group with (0, 0) exactly as the original.
