# Battle view port status (GRAPHICS)

Owned files: `crates/s2engine/src/battle/view*.rs`.

## Map surface bookkeeping (`view_surface.rs`)

The map surface (`DAT_004947f0`) is persistent and only changes the way the
original's does:

| original | port |
|---|---|
| `FUN_00442022(army, unit, mode)` capture (+ `FUN_00442238`, `FUN_004423d3`) | `view::capture` / `capture_raw`, `Surf::capture`, `unit::capture` |
| `FUN_0046f907` grab (zeroes above / first-row-left, keeps right / below) | `unit::grab_into` |
| `FUN_00440264(army, unit, full)` | `view::put_unit`, `Surf::put` |
| `FUN_00441c0b` + `FUN_00441b15` x2 | `view::erase_unit`, `Surf::erase` |
| `FUN_004403b5` | `view::bake_army`, `Surf::bake_army` |
| `FUN_00441222` / `FUN_00441ef1` | `Surf::erase_army` / `grab_army` |
| `FUN_00431845` / `FUN_0040fa1d` (bg, grab all, bake all) | `view::reset_surface` + `Surf::rebuild` (also after `init`) |
| `FUN_004231be` | `view::redeploy` |

Each unit keeps its save-under (`UnitGfx::under`, the cell it was captured
on) and what it last drew (`drawn`). The surface changes **only** by the
explicit operations, called where the original calls them; `view::bake`
just syncs sprites and rebuilds after `reset_surface` / a new map. Shadows,
hit shake and invisibility use the stored save-under. Scene draws add the
lifted units (act 1/3/5) that are not in the surface on top.

Wired call sites (original → port):

- AI: FUN_0040a4d0 lift erase (`ai::decide`), FUN_0044e210 (`turn::ai_finish`),
  FUN_0040a443 (`turn::ai_act`), FUN_00440e71 (`turn::phase_reset` → bake_army),
  FUN_00441038 (`turn::end_turn`).
- Player menu: FUN_004346cb erase/put (`menu::select_of`), FUN_00434893 exits,
  undo-move capture, cast erase (`command_loop` / `finish`), FUN_0044c240 capture
  (`move_command`), FUN_0044d7c1 rest erase+put (`rest_command`), FUN_0044fb84
  (`show_unit_range`).
- Moves: FUN_0044c331 per-step captures (`view::animate_walk`), FUN_0044c963
  (`animate_teleport`), FUN_00458334 (`turn::teleport`), FUN_0045844b (`hooks::free_move`).
- Combat: FUN_00436e46 / FUN_00437b9c page 2 = window + FUN_00441e09 on-page
  erases (`actions::attack` / `counters`), counter-attacker erase/put, victim
  FUN_00441c0b before FUN_0044d14b, FUN_00438c92 petrify put
  (`actions::apply_on_hit`), FUN_0044d14b on-page erase + FUN_0044007a
  (`actions::die`).
- Spells: FUN_00466ba5 partner erase/put (`view::cast::caster_pose`), FUN_0045ff9e
  combo puts (cast.rs), FUN_0046136c / FUN_00462144 puts (`Show::Put`),
  FUN_00460fd0 opponent bake (`Show::Redraw`), FUN_004608e2 (`summon_rise`), deaths.
- Events: FUN_00440264 / FUN_00441c0b hooks, FUN_0043d9e9 creation capture,
  FUN_004588f2 / FUN_00458c60 / FUN_00458ff3 walks, FUN_00459131 warp (no put,
  as in the original), FUN_0045bc6a cases, FUN_00459214 army switch,
  FUN_0045c37d group move, type-15 group action, FUN_00417d8e talk,
  FUN_0045d922, FUN_0041be7a (`rebake_armies`), FUN_0044cf97 withdraw.
- Turn: FUN_0045ad90 upkeep, FUN_0045b29d terrain, FUN_0045d713 victory EXP,
  FUN_004231be (`redeploy`), load (`reset_surface`), FUN_0042deda
  (`redraw_armies`), background / chest changes (`rebake_armies`).
- FLOW deployment screens rebuild the surface (`reset_surface`) at each step.

Captures (FUN_00442238) and the units-behind pass (FUN_004423d3) take the
unit's own army first, then the other, as the original passes them; the
order shows where those images overlap (stage 12 / 102 death dissolves).

`view::play_actions` (FUN_0044ecf8) no longer lifts the units: the window
shows the surface as it is under the units' new frames (the residue the
original leaves, e.g. spells_r0_005 t4_01–04).

Debugging: `S2_SURFACE_DUMP=DIR:MS` writes the surface and every unit's
save-under after MS; `oracle.py --peek 80494800:W*H` dumps the original's
(`W, H` at `0x4947f0`); `S2_BATTLE_TRACE=1` logs every capture / paste / put.

## Oracle (2026-10-08)

Full check.sh: 1370/1379 before the explicit-call port, 1379/1379 after
(every script pixel- and file-identical).
