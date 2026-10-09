//! Two-player battles (雙人對戰): loading the battlefield and the two armies
//! from save files (`FUN_0040f67f`, `FUN_0040f418`, `FUN_00443482`) and the
//! phase of a human side (`FUN_0043297f`). Owned by the AI port.
//!
//! VS API
//! ------
//! ```text
//! stage_list(data) -> Result<Vec<name>>     FUN_0040f1aa: the battlefields of FSTAGE.BIN (the setup lists "%2d.%s"
//!                                           with the 1-based index)
//! start(game, stage, blue, red) -> Result<VsTotals>
//!                                           FUN_0040f67f: sets vs_mode; loads FSTAGE stage `stage` (out of range → 1)
//!                                           with its script; turn 1, difficulty 5, no next/alternate stage, the stage
//!                                           music; splits the records; map; blue army = the player army of save slot
//!                                           `blue` (1-based) on the blue records, red likewise; the comparison screen.
//!                                           Then the stage loop runs as for a new stage (FUN_0040d108(10)).
//! phase(game, side) -> i32                  FUN_0043297f: one human side's phase. 0 done, 1 blue won (red wiped
//!                                           out), 2 red won, 0xb quit.
//! load_army(b, side, &ArmyRecord)           FUN_00443482 (read part): an army as a save stores it
//! ```
//!
//! What the setup (FLOW, `FUN_0040e5db`) provides: a battlefield from
//! `stage_list` and two save slots chosen with `flow::save::pick_army_save`
//! (blue = side 0, red = side 1; 確定 only once both are set). Each army is
//! its save's *player* army: units, gold, flags, items and summon slots; the
//! save's difficulty byte appears on the comparison screen only. Clear the
//! story flags (the title does on code 10) and call `start`.
//!
//! What the stage loop (FLOW, `FUN_0040d108`) does differently in VS: no
//! per-turn deployment or autosaves (`vs_side = 0` instead); the blue phase
//! is `phase(game, Side::Player)` followed by `vs_side = 1`, the red phase
//! `phase(game, Side::Enemy)` followed by `vs_side = 0` (in place of
//! `menu::player_phase` / `turn::enemy_phase`); on 1 / 2 / 0xb the box
//! 藍軍勝利 / 紅軍勝利 (UI strings 0x10c / 0x10d, only for 1 / 2), fade out,
//! `vs_mode = 0`, return 0x14 to the title. All of that is in
//! `flow/stage.rs` already except the two phase calls.
//!
//! The VS rules live where the campaign ones do: no difficulty scaling or
//! EXP multiplier (`rules::Ctx::vs_mode`), kills drop the victim's equipment
//! (`kill_bonus`, spells), created units at level 1, no class change
//! (`promotion_available`), no NPC proficiency floor, no caves, treasure,
//! tile flags or trigger tiles, hidden units hidden only from the side not
//! acting (`Battle::vs_side`), victory = the other army wiped out
//! (`event::check_defeat` / `check_victory`).

use anyhow::Result;
use s2data::save::{ArmyRecord, SaveFile};
use s2data::tbl_stage::StageFile;

use super::actions::presenting;
use super::army::{Side, SummonSlot};
use super::camera;
use super::menu;
use super::turn;
use super::unit::{Unit, UNIT_SIZE};
use super::Battle;
use crate::game::Game;
use crate::{cursor, gfx, text, ui};

/// The comparison screen of `start` (`DAT_004944c0..dc`): per side the
/// save's difficulty, the deployed units' power sum and maximum, and their number.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VsTotals {
    pub save_difficulty: [i32; 2],
    pub power_sum: [i32; 2],
    pub power_max: [i32; 2],
    pub count: [i32; 2],
}

/// Port of FUN_0040f1aa: the battlefield names of FSTAGE.BIN.
pub fn stage_list(data: &crate::Data) -> Result<Vec<Vec<u8>>> {
    let b = data.dir.read(s2data::tables::source::VS_STAGES)?;
    Ok(s2data::script::Stage::parse_all(&b)?.into_iter().map(|s| s.name).collect())
}

/// Port of FUN_00443482 (reading part; FUN_0044306f's layout): army `side`
/// becomes `rec`. The placement on the stage records follows in `start`.
pub fn load_army(b: &mut Battle, side: Side, rec: &ArmyRecord) {
    let a = b.army_mut(side);
    a.units = rec
        .units
        .iter()
        .map(|u| {
            let mut raw = [0u8; UNIT_SIZE];
            let n = u.0.len().min(UNIT_SIZE);
            raw[..n].copy_from_slice(&u.0[..n]);
            Unit::from_bytes(&raw)
        })
        .collect();
    a.gold = rec.gold as i32;
    a.flags = rec.flags;
    a._d6 = rec.d6 as i32;
    a._d4 = rec.d4 as i16;
    a.stat_bonus_pct = rec.scale;
    a.inventory = rec.items.clone();
    for (i, s) in a.summons.iter_mut().enumerate() {
        if let Some(t) = rec.tail.get(i * 3..i * 3 + 3) {
            *s = SummonSlot { active: t[0], kind: i16::from_le_bytes([t[1], t[2]]) };
        }
    }
}

/// The save in slot `slot` (1-based, `save\uj%02d.sav`).
fn read_save(game: &Game, slot: usize) -> Result<SaveFile> {
    let n_items = game.battle.t().items.len();
    let b = game.data.dir.read(&crate::flow::save::slot_path(slot.saturating_sub(1)))?;
    Ok(SaveFile::parse(&b, n_items)?)
}

/// Port of FUN_0040f67f (with FUN_0040f418 and FUN_00443482): set up a
/// two-player battle on FSTAGE battlefield `stage` between the player
/// armies of save slots `blue` and `red` (1-based).
#[allow(clippy::needless_range_loop)] // index math as in the original's loop
pub fn start(game: &mut Game, stage: usize, blue: usize, red: usize) -> Result<VsTotals> {
    // FUN_0040e5db sets DAT_00494668 right before.
    game.campaign.vs_mode = true;
    game.battle.load_tables(game.data)?;
    // FUN_0040f418: the battlefield record (out of range → 1) and its script.
    let bytes = game.data.dir.read(s2data::tables::source::VS_STAGES)?;
    let file = StageFile::parse(bytes.clone())?;
    let id = if stage == 0 || stage > file.len() { 1 } else { stage };
    let bstage = file.stage(id)?;
    let sstage = s2data::script::Stage::parse(&bytes, id)?;
    // The script is attached (FUN_00417556) without the campaign's header flags.
    let flags = game.campaign.flags;
    crate::event::attach_stage(game, &sstage, false);
    game.campaign.flags = flags;
    game.ui.place_name = sstage.name.clone();
    let c = &mut game.campaign;
    c.stage = id as u16;
    c.turn = 1;
    c.alt_next_stage = 0;
    c.cave_found = false;
    c.difficulty = 5;
    c.music = sstage.header.music;
    c.next_stage = 0;
    c.between_stages = false;
    for t in game.events.treasure.iter_mut().take(5) {
        *t = [0; 4];
    }
    game.events.treasure_count = 0;
    crate::flow::save::sync_battle_mirrors(game);
    game.battle.vs_side = 0;
    game.battle.stage_kind = sstage.header.kind;
    game.stage = Some(crate::flow::stage::StageState { stage: sstage, battle_stage: bstage.clone() });
    // FUN_00424d81
    crate::flow::stage::map_init(game)?;
    // FUN_00443482 ×2: blue on the records before the first side-2/3/8 record, red on the rest.
    let saves = [read_save(game, blue)?, read_save(game, red)?];
    load_army(&mut game.battle, Side::Player, &saves[0].player);
    game.battle.start_stage_placed(&bstage);
    load_army(&mut game.battle, Side::Enemy, &saves[1].player);
    game.battle.place_party(Side::Enemy);
    game.battle.recalc_army_flags(Side::Enemy, 1);
    let mut tot = VsTotals::default();
    for k in 0..2 {
        tot.save_difficulty[k] = saves[k].difficulty() as i8 as i32;
        let (sum, max, n) = game.battle.power_totals[k];
        tot.power_sum[k] = sum;
        tot.power_max[k] = max;
        tot.count[k] = n;
    }
    // FUN_0043f693 / FUN_004433de / FUN_004403b5: sprites and the redraw.
    crate::flow::stage::after_load(game);
    comparison_screen(game, &tot);
    Ok(tot)
}

/// The comparison screen at the end of FUN_0040f67f: both sides' save
/// difficulty, power sum, best power and unit count; wait for a click,
/// fade out.
fn comparison_screen(game: &mut Game, t: &VsTotals) {
    if !presenting(game) {
        return;
    }
    game.scr.clear();
    let (m1, m2, m3) = (game.msg(0x109), game.msg(0x10a), game.msg(0x10b));
    text::large(game, 0x122, 0x32, &m1, 0xaf, 0, 1, 2);
    text::large(game, 0x1b8, 0x32, &m2, 0xaf, 0, 1, 2);
    text::block(game, 0x5a, 100, &m3, 0xaf, 0, 1, 2, 0x32, 0x14, 0x1c);
    for (k, x) in [(0usize, 300), (1, 0x1c2)] {
        let rows = [t.save_difficulty[k], t.power_sum[k], t.power_max[k], t.count[k]];
        for (i, v) in rows.into_iter().enumerate() {
            text::large(game, x, 100 + 0x1c * i as i32, v.to_string().as_bytes(), 0xaf, 0, 1, 2);
        }
    }
    gfx::present(game);
    ui::click_to_continue(game, 0);
    let m = game.res().master.clone();
    gfx::fade(game, &m, 1, 0);
}

/// Port of FUN_0043297f(side): the phase of a human side in a VS battle —
/// the player phase loop (FUN_004323a3) for army `side`, without saving,
/// loading or retreating, the "actions" event poll or the difficulty-5
/// range rule. Returns 0 when every unit has acted (after the side's NPC
/// and berserk units), 1 / 2 from the stage check, 0xb to quit.
pub fn phase(game: &mut Game, side: Side) -> i32 {
    super::sync(game);
    let mut redraw_status = true;
    let mut cycle = 1usize;
    let mut sys = 0;
    let mut auto = 1;
    camera::redraw_full(game);
    loop {
        super::actions::pump(game);
        if !game.cfg.auto_switch {
            auto = 0;
        }
        if auto == 1 && menu::next_unit_of(game, side, &mut cycle) == 0 {
            auto = 0;
        }
        let (mut x, mut y) = (0, 0);
        let code = menu::input(game, &mut x, &mut y, 0);
        match code {
            0 => {
                if redraw_status {
                    menu::status_redraw(game);
                    redraw_status = false;
                }
            }
            -0x21 => {
                if presenting(game) {
                    cursor::draw_present(game, None, true);
                }
            }
            -0x20 | -0x1f | -8 | -3 => {
                let (cx, cy) = game.battle.cursor;
                match menu::select_of(game, side, cx, cy, None, code) {
                    3 => sys = menu::system_menu(game),
                    0 => auto = 0,
                    1 => auto = 1,
                    2 => {
                        if !menu::unit_info(game, side, cx, cy) {
                            menu::unit_info(game, side.other(), cx, cy);
                        }
                    }
                    4
                        if menu::next_unit_of(game, side, &mut cycle) == 0 => {
                            auto = 0;
                        }
                    _ => {}
                }
                redraw_status = true;
            }
            -0x1e => game.cfg.sfx_volume = (game.cfg.sfx_volume + 1).min(100),
            -0x1d => game.cfg.sfx_volume = (game.cfg.sfx_volume - 1).max(0),
            -0x1a => return turn::auto_phase(game, side),
            -0x19 => turn::end_turn(game, side),
            -0x18 => {
                game.cfg.status_window = !game.cfg.status_window;
                menu::status_redraw(game);
            }
            -0x17 => {
                let (cx, cy) = game.battle.cursor;
                if !menu::unit_info(game, side, cx, cy) {
                    menu::unit_info(game, side.other(), cx, cy);
                }
                menu::status_redraw(game);
            }
            -0x16 => {
                crate::flow::status::map_overview(game);
                menu::status_redraw(game);
            }
            -0x15 => {
                if menu::next_unit_of(game, side, &mut cycle) == 0 {
                    auto = 0;
                }
            }
            -0x14 | -0x13 => {
                let d = if code == -0x14 { 1 } else { -1 };
                game.cfg.midi_volume = (game.cfg.midi_volume + d).clamp(0, 100);
                game.cfg.cd_volume = (game.cfg.cd_volume + d).clamp(0, 100);
                let v = game.cfg.midi_volume as u8;
                game.sys.audio.set_midi_volume(v);
            }
            -0x12 => {
                if presenting(game) {
                    gfx::rect(game.scr.page(), 0, 0, 0x27f, 0x14, 0xfe, true);
                    text::small(game, 0, 2, b"The SuperSp Game V1.4 Copyright (c) 1997 by U&J TECHNOLOGY CO., LTD.", 0x2a, 0, 1, 2);
                    gfx::present(game);
                }
            }
            -0x11 => return 0xb,
            -6 => redraw_status = true,
            -5 => {
                // FUN_00432ecf(1): edge scrolling with the status redraw.
                menu::follow_cursor(game, &mut menu::status_redraw);
                redraw_status = true;
            }
            -4 | 0x1c => {
                let (cx, cy) = game.battle.cursor;
                if !menu::show_unit_range(game, side, cx, cy) && !menu::show_unit_range(game, side.other(), cx, cy) {
                    game.battle.map_mut().center_on(cx, cy, true);
                }
                redraw_status = true;
            }
            -1 => {
                sys = menu::system_menu(game);
                redraw_status = true;
            }
            c => {
                // Everything else, save (−0x1c) and load (−0x1b) included: map scrolling.
                if game.battle.map_mut().scroll_key(c) {
                    redraw_status = true;
                    let t = game.ticks();
                    menu::keep_cursor_in_view(game);
                    camera::redraw_full(game);
                    let s = game.cfg.screen_speed;
                    game.wait_until(t, s);
                }
            }
        }
        match sys {
            3 => turn::end_turn(game, side),
            6 => return turn::auto_phase(game, side),
            0xb => return 0xb,
            _ => {}
        }
        sys = 0;
        let c = turn::stage_check(game);
        if c == 1 || c == 2 {
            return c;
        }
        if cycle == 0 || turn::all_done(&game.battle, side) {
            break;
        }
    }
    turn::npc_phase(game, side)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::unit::flag;
    use crate::sys::InputEvent;

    /// VK_F5 (scan 0x3f, 自動作戰).
    const F5: u8 = 0x74;

    fn save_slots(d: &crate::Data) -> Option<(usize, usize)> {
        let ok = |n: usize| d.dir.read(&crate::flow::save::slot_path(n - 1)).is_ok();
        (ok(1) && ok(2)).then_some((1, 2))
    }

    #[test]
    fn vs_stage_list_reads_fstage() {
        let Some(d) = crate::battle::spells::tests::data() else { return };
        let names = stage_list(&d).unwrap();
        eprintln!("{} VS battlefields: {:?}", names.len(), names.iter().map(|n| s2data::text::big5(n)).collect::<Vec<_>>());
        let b = d.dir.read(s2data::tables::source::VS_STAGES).unwrap();
        let st = s2data::script::Stage::parse_all(&b).unwrap();
        eprintln!("events per battlefield: {:?}", st.iter().map(|s| s.events.len()).collect::<Vec<_>>());
        assert_eq!(names.len(), 10);
        assert!(names.iter().all(|n| !n.is_empty()));
    }

    /// Battlefield 1 between saves 1 and 2: both armies are placed on their
    /// records, the globals are set, and a phase of each side played by the
    /// AI (F5 = 自動作戰) is deterministic.
    #[test]
    fn vs_start_and_auto_phases() {
        let Some(d) = crate::battle::spells::tests::data() else { return };
        let Some((blue, red)) = save_slots(&d) else { return };
        let run = || {
            crate::battle::ai::reset_state();
            let shared = crate::sys::Shared::new();
            let mut sys = crate::sys::Sys::new(shared.clone(), crate::audio::Audio::silent());
            let mut game = Game::new(&mut sys, &d);
            game.events.ui = crate::event::ui::UiHooks::headless();
            game.cfg.auto_switch = false;
            let tot = start(&mut game, 1, blue, red).unwrap();
            game.battle.rng = crate::battle::rng::Rng::started(crate::battle::rng::Seeder::fixed(5));
            assert!(game.campaign.vs_mode && game.battle.vs);
            assert_eq!((game.campaign.turn, game.campaign.difficulty, game.campaign.next_stage), (1, 5, 0));
            let b = &game.battle;
            for (k, side) in [Side::Player, Side::Enemy].into_iter().enumerate() {
                let (start, n) = if k == 0 { (0, b.n_player_defs) } else { (b.enemy_defs_start, b.n_enemy_defs) };
                let placed: Vec<(i32, i32)> =
                    b.army(side).units.iter().filter(|u| u.flags & flag::DEPLOYED != 0).map(|u| (u.x, u.y)).collect();
                assert_eq!(placed.len() as i32, tot.count[k]);
                assert!(placed.len() <= n && !placed.is_empty(), "side {k}: {} on {n} records", placed.len());
                for (i, &(x, y)) in placed.iter().enumerate() {
                    let p = &b.defs[start + i].p;
                    assert_eq!((x, y), (p.x as i32, p.y as i32));
                }
                assert!(tot.power_max[k] > 0 && tot.power_sum[k] >= tot.power_max[k]);
            }
            eprintln!("VS totals {tot:?}");
            // Blue's phase, then red's, both on auto battle.
            let mut codes = Vec::new();
            for side in [Side::Player, Side::Enemy] {
                shared.push_input(InputEvent::KeyDown(F5));
                shared.push_input(InputEvent::KeyUp(F5));
                game.battle.vs_side = side.index() as i32;
                codes.push(phase(&mut game, side));
            }
            let state: Vec<(i32, i32, i16, u8)> = [Side::Player, Side::Enemy]
                .into_iter()
                .flat_map(|s| game.battle.army(s).units.iter().map(|u| (u.x, u.y, u.hp, u.act)).collect::<Vec<_>>())
                .collect();
            (codes, state, game.battle.rng.snapshot())
        };
        let a = run();
        let b = run();
        eprintln!("VS phase codes {:?}", a.0);
        assert!(a.0.iter().all(|&c| (0..=2).contains(&c)));
        assert_eq!(a, b, "deterministic");
        // Every deployed unit that could act has acted.
        assert!(a.1.iter().any(|u| u.3 == 2));
    }

    #[test]
    fn army_record_round_trip() {
        let Some(d) = crate::battle::spells::tests::data() else { return };
        let Some((blue, _)) = save_slots(&d) else { return };
        let mut b = crate::battle::spells::tests::stage1().unwrap();
        let n_items = b.t().items.len();
        let bytes = d.dir.read(&crate::flow::save::slot_path(blue - 1)).unwrap();
        let save = SaveFile::parse(&bytes, n_items).unwrap();
        load_army(&mut b, Side::Enemy, &save.player);
        let a = b.army(Side::Enemy);
        assert_eq!(a.units.len(), save.player.units.len());
        for (u, r) in a.units.iter().zip(&save.player.units) {
            assert_eq!(&u.to_bytes()[..0xad], &r.0[..0xad]);
        }
        assert_eq!(a.gold, save.player.gold as i32);
        assert_eq!(a.inventory, save.player.items);
    }
}
