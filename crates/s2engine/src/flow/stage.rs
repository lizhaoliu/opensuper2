//! Stage setup, the map view glue and the stage loop frame (`FUN_0040d108`).
//!
//! The map object (`DAT_00497a68`) and the armies are BATTLE's
//! (`game.battle`), drawing is the VIEW API (`crate::battle::view`).
//! The battle turns of the stage loop call the BATTLE API.

use anyhow::{Context, Result};
use s2data::script::Stage;
use s2data::tbl_stage::{Stage as BattleStage, StageFile};

use super::code;
use crate::battle;
use crate::battle::map::Map;
use crate::battle::unit::flag;
use crate::battle::{view, Side, UnitRef};
use crate::cursor;
use crate::event::{self, Trigger};
use crate::game::Game;
use crate::gfx;

/// The stage being played: the scenario (events, texts) and the battle
/// placement records.
pub struct StageState {
    pub stage: Stage,
    pub battle_stage: BattleStage,
}

/// Port of `FUN_00424946`: drop the stage, map and armies' map state
/// (`FUN_0042318e` script and `FUN_00424ee0` map release are Rust drops).
pub fn reset(game: &mut Game) {
    event::detach(game);
    game.stage = None;
}

/// Port of `FUN_00423033(stage, mode)` and the stage-record part of
/// `FUN_00422b8e`. Mode 10 (fresh start) sets turn 1 and the next stages;
/// mode 7 (from a save) keeps the campaign globals.
pub fn load_stage(game: &mut Game, id: u16, mode: i32) -> Result<()> {
    let stage = Stage::parse(&game.res().stages, id as usize).with_context(|| format!("stage {id}"))?;
    let battle_stage = StageFile::parse(game.res().stages.clone())?.stage(id as usize)?;
    if mode == 10 {
        let c = &mut game.campaign;
        c.stage = id;
        c.turn = 1;
        let alt = stage.header.secret_next[0] as u16;
        c.alt_next_stage = alt;
        c.cave_found = false;
        c.music = stage.header.music;
        if stage.header.kind == 2 {
            if stage.header.secret_next[0] == 0xff {
                c.next_stage = stage.header.next;
                c.alt_next_stage = 0;
            } else if stage.header.next == alt {
                c.next_stage = stage.header.next;
            }
        } else {
            c.next_stage = stage.header.next;
        }
    }
    event::attach_stage(game, &stage, mode == 10);
    game.battle.load_tables(game.data)?;
    super::save::sync_battle_mirrors(game);
    if mode == 10 {
        // The record split and the enemy army; FUN_00423033 places nobody
        // (the deployment / records / cave decide who is on the map).
        game.battle.start_stage(&battle_stage);
        // FUN_0043f42c: the party's sprite slots and the records' types;
        // in a kind-3 stage the named members take their record's facing
        // and every member (not dead, not summoned) its idle action
        // (FUN_00441ac4(army, unit, 0)).
        game.battle.slots_stage_start();
        if stage.header.kind == 3 {
            for i in 0..game.battle.defs.len() {
                let p = game.battle.defs[i].p.clone();
                if p.character >= 1 {
                    if let Some(k) = game.battle.army(Side::Player).find_char(p.character, 1) {
                        game.battle.army_mut(Side::Player).units[k].facing = p.facing;
                    }
                }
            }
            for k in 0..game.battle.army(Side::Player).units.len() {
                let f = game.battle.army(Side::Player).units[k].flags;
                if f & (crate::battle::unit::flag::DEAD | crate::battle::unit::flag::SUMMON) == 0 {
                    view::set_anim(game, UnitRef::new(Side::Player, k), 0);
                }
            }
        }
    } else {
        // From a save: only the record split (the armies are the save's).
        super::armies::set_records(game, &battle_stage);
    }
    game.battle.stage_kind = stage.header.kind;
    game.stage = Some(StageState { stage, battle_stage });
    Ok(())
}

/// Port of `FUN_00424d81`: load the map and build its surface (VIEW).
pub fn map_init(game: &mut Game) -> Result<()> {
    super::stop_music(game);
    let id = game.stage.as_ref().context("no stage loaded")?.stage.header.map as usize;
    game.battle.set_map(Map::load(&game.data.arcs, id)?);
    view::init(game, id)?;
    Ok(())
}

/// Port of `FUN_004231be` (`FUN_004233c7`): deploy this turn's records, then
/// redraw the idle units into the map surface.
pub fn deploy_turn_units(game: &mut Game) {
    super::save::sync_battle_mirrors(game);
    let turn = game.campaign.turn as i32;
    if game.battle.deploy_turn(turn) {
        view::redeploy(game);
    }
}

/// After a save was loaded inside a stage (`FUN_004433de`, `FUN_004403b5`):
/// reload the unit sprites and redraw the surface.
pub fn after_load(game: &mut Game) {
    // FUN_004433de skips dead units (FUN_0044306f gave them their frame with
    // FUN_00443c84, which starts no action): they keep their frame byte.
    for r in super::save::all_units(game) {
        if game.battle.unit(r).flags & flag::DEAD == 0 {
            view::reload_sprite(game, r);
        }
    }
    // FUN_00431845: the map surface rebuilt (FUN_00424d81) first.
    view::reset_surface(game);
    view::bake(game);
}

/// Port of `FUN_00442da4`: centre the view on the army's first live,
/// deployed unit.
pub fn center_on_first_unit(game: &mut Game) {
    let first = game
        .battle
        .army(Side::Player)
        .units
        .iter()
        .find(|u| u.flags & flag::DEAD == 0 && u.flags & flag::DEPLOYED != 0)
        .map(|u| (u.x, u.y));
    if let (Some((x, y)), Some(m)) = (first, game.battle.map.as_mut()) {
        m.center_on(x, y, true);
        game.battle.cursor = (x, y);
    }
}

/// Port of `FUN_00424c5f`: the 640x480 view of the map surface into the current page.
pub fn draw_map(game: &mut Game) {
    let (sx, sy) = match game.battle.map.as_ref() {
        Some(m) => (m.scroll_x, m.scroll_y),
        None => return,
    };
    let Some(mv) = game.battle.view.map.as_mut() else { return };
    mv.scroll_x = sx;
    mv.scroll_y = sy;
    let cur = game.scr.cur;
    mv.blit_window(&mut game.scr.pages[cur]);
}

/// Port of `FUN_004252c9`: draw the view on page 0, then the cursor, and present.
pub fn draw_map_present(game: &mut Game) {
    game.scr.set_page(0);
    draw_map(game);
    cursor::draw_present(game, None, false);
}

/// Port of `FUN_00426905(map, col, row, 0, 0)`: scroll one step per frame
/// (畫面速度 cs apart) until the cell is well inside the view; with 快速切換
/// the camera jumps first.
pub fn scroll_to(game: &mut Game, col: i32, row: i32) {
    let Some(m) = game.battle.map.as_mut() else { return };
    let h = m.height(col, row);
    let mut c = if row & 1 == 0 { col * 2 - 9 } else { col * 2 - 8 };
    let mut r = row - (h + 0xf);
    if m.width() <= (c + 0x15) >> 1 {
        c = m.width() * 2 - 0x15;
    }
    if m.rows() <= r + 0x1e {
        r = m.rows() - 0x1e;
    }
    c = c.max(1);
    r = r.max(1);
    if game.cfg.quick_switch {
        m.scroll_x = c;
        m.scroll_y = r;
    }
    let mut seen = 0;
    loop {
        let m = game.battle.map.as_mut().expect("map");
        m.scroll_y += (r - m.scroll_y).signum();
        m.scroll_x += (c - m.scroll_x).signum();
        let t = game.ticks();
        draw_map(game);
        gfx::present(game);
        let speed = game.cfg.screen_speed;
        game.wait_until(t, speed);
        super::music_keepalive(game);
        gfx::palette_cycle(game);
        let m = game.battle.map.as_ref().expect("map");
        let arrived = m.scroll_x == c && m.scroll_y == r;
        if m.on_screen(col, row, 0) {
            seen += 1;
            if seen == 4 {
                break;
            }
        }
        if arrived {
            break;
        }
    }
}

/// Port of `FUN_00442caa(army, char_id, &x, &y)` over the player army, then
/// the enemy army, then (as every caller does) the player army's character 1:
/// the speaker's cell, or the army's first unit when the speaker is not on
/// the map; scrolls it into view and returns its screen position.
pub fn unit_screen_pos(game: &mut Game, char_id: i16) -> (bool, i32, i32) {
    let lookup = |game: &Game, side: Side, id: i16| -> Option<(bool, i32, i32)> {
        let a = game.battle.army(side);
        let u = a.units.iter().find(|u| u.char_id == id)?;
        if u.flags & flag::DEAD == 0 && u.flags & flag::DEPLOYED != 0 {
            Some((true, u.x, u.y))
        } else {
            let f = a.units.first()?;
            Some((false, f.x, f.y))
        }
    };
    let hit = lookup(game, Side::Player, char_id).or_else(|| lookup(game, Side::Enemy, char_id));
    let (found, x, y) = match hit {
        Some(h) => h,
        None => match lookup(game, Side::Player, 1).or_else(|| game.battle.army(Side::Player).units.first().map(|u| (false, u.x, u.y))) {
            Some((_, x, y)) => (false, x, y),
            None => return (false, 0, 0),
        },
    };
    let visible = game.battle.map.as_ref().is_some_and(|m| m.on_screen(x, y, 2));
    if !visible {
        scroll_to(game, x, y);
    }
    let (sx, sy) = game.battle.map.as_ref().map_or((0, 0), |m| m.cell_to_screen(x, y));
    (found, sx, sy)
}

/// `FUN_0043a860(army, char, 0)` on both armies: is the character in either
/// army, not in reserve unless deployed.
pub fn in_an_army(game: &Game, char_id: i16) -> bool {
    use crate::battle::unit::flag;
    [Side::Player, Side::Enemy].iter().any(|&s| {
        game.battle.army(s).units.iter().any(|u| u.char_id == char_id && (u.flags & flag::RESERVE == 0 || u.flags & flag::DEPLOYED != 0))
    })
}

/// Units of an army, for screens that list them.
pub fn units_of(game: &Game, side: Side) -> Vec<UnitRef> {
    (0..game.battle.army(side).units.len()).map(|i| UnitRef::new(side, i)).collect()
}

/// Port of `FUN_0040d108`: one stage from setup to result. The battle
/// turns call the BATTLE API (`battle::turn`, `battle::menu`).
pub fn stage_loop(game: &mut Game, c: i32) -> Result<i32> {
    if c == code::QUIT {
        return Ok(code::QUIT);
    }
    let master = game.res().master.clone();
    if c == code::LOADED || c == code::LOADED_SLOT50 {
        gfx::fade(game, &master, 1, 0);
    }
    let kind = game.stage.as_ref().map(|s| s.stage.header.kind).context("stage loop without a stage")?;
    if kind == 3 {
        // Story-only stage.
        deploy_turn_units(game);
        center_on_first_unit(game);
        draw_map_present(game);
        gfx::fade(game, &master, 0, 0);
        game.campaign.music = 0;
        let cue = game.stage.as_ref().map(|s| s.stage.header.music).unwrap_or(0);
        super::music(game, cue);
        event::poll(game, Trigger::TurnStart);
        if game.campaign.next_stage == 0 {
            event::poll(game, Trigger::FlagChain { unit_context: false });
        } else if let Some(st) = game.stage.as_ref() {
            // FUN_0045cfad when the stage has rewards other than a lone treasure list.
            let n = st.stage.header.reward_groups;
            let first_treasure = matches!(st.stage.rewards.first(), Some(s2data::script::Reward::Treasure(_)));
            if n > 1 || (n == 1 && !first_treasure) {
                battle::turn::victory_rewards(game);
            }
        }
        super::stop_music(game);
        event::clear_stage_flags(game);
        gfx::fade(game, &master, 1, 0);
        return Ok(if game.campaign.next_stage == 0 { code::CAMPAIGN_DONE } else { code::WIN });
    }
    // From here the battle loop of FUN_0040d108; the phases are the BATTLE API
    // (`crate::battle::{turn, menu}`).
    battle::turn::adapt_difficulty(game);
    let fame = game.battle.recalc_army_flags(Side::Player, 1);
    battle::turn::pay_fame(game, fame);
    game.campaign.location = game.campaign.stage;
    if kind == 1 && c != code::LOADED && c != code::LOADED_SLOT50 {
        game.campaign.battles_fought += 1;
    }
    let mut c = c;
    let r = loop {
        super::music_keepalive(game);
        gfx::palette_cycle(game);
        battle::sync(game);
        if !game.campaign.vs_mode {
            if c == code::FROM_WORLD_MAP {
                game.campaign.music = game.stage.as_ref().map(|s| s.stage.header.music).unwrap_or(0);
                autosave_turn1(game)?;
            } else if c != code::LOADED && c != code::LOADED_SLOT50 {
                deploy_turn_units(game);
                autosave_turn1(game)?;
            }
        } else {
            game.battle.vs_side = 0;
        }
        let loaded = c == code::LOADED || c == code::LOADED_SLOT50;
        let mut r = 0;
        let mut run_turn = true;
        if game.campaign.turn == 1 || loaded {
            center_on_first_unit(game);
            draw_map_present(game);
            gfx::fade(game, &master, 0, 0);
            let cue = if loaded { game.campaign.music } else { game.stage.as_ref().map(|s| s.stage.header.music).unwrap_or(0) };
            if loaded || cue != 0 {
                game.campaign.music = 0;
                super::music(game, cue);
            }
            if game.battle.bg_failed_notice {
                // DAT_0048e944 == 0: a background failed to load (see FUN_0042b030).
                game.battle.bg_failed_notice = false;
                let m = game.msg(0x103);
                crate::ui::banner(game, 0x140, 100, &m, crate::ui::Wait::Click(0));
            }
        } else {
            r = battle::turn::turn_upkeep(game);
            run_turn = r == 0;
        }
        if run_turn {
            if c != code::LOADED {
                event::poll(game, Trigger::TurnStart);
                r = battle::turn::stage_check(game);
            }
            if r == 0 {
                if c == code::FROM_WORLD_MAP {
                    autosave_turn(game)?;
                    c = code::NEW_STAGE;
                } else if !loaded {
                    autosave_turn(game)?;
                }
                battle::turn::phase_begin(game, Side::Player);
                if !loaded {
                    battle::turn::phase_banner(game, 0);
                }
                if game.campaign.turn == 1 {
                    draw_map(game);
                    battle::turn::objectives_screen(game);
                }
                battle::turn::wake_ai(game, Side::Player);
                if loaded {
                    c = code::NEW_STAGE;
                }
                if game.campaign.vs_mode {
                    // FUN_0043297f(0): the blue player's phase.
                    r = battle::vs::phase(game, Side::Player);
                    game.battle.vs_side = 1;
                } else {
                    r = battle::menu::player_phase(game);
                }
            }
        }
        // LAB_0040d44d
        if matches!(r, code::QUIT | code::LOADED | code::LOADED_SLOT50 | code::LOSE | code::RETREAT) {
            break r;
        }
        battle::turn::phase_reset(game, Side::Player);
        if r == code::WIN {
            event::poll(game, Trigger::Victory);
            break r;
        }
        r = battle::turn::phase_terrain(game, Side::Player);
        if r == code::LOSE || r == code::RETREAT {
            break r;
        }
        if r == code::WIN {
            event::poll(game, Trigger::Victory);
            break r;
        }
        event::poll(game, Trigger::EnemyPhaseEnd);
        battle::turn::phase_begin(game, Side::Enemy);
        battle::turn::phase_banner(game, 1);
        battle::turn::wake_ai(game, Side::Enemy);
        if game.campaign.vs_mode {
            // FUN_0043297f(1): the red player's phase.
            r = battle::vs::phase(game, Side::Enemy);
            game.battle.vs_side = 0;
        } else {
            r = battle::turn::enemy_phase(game);
        }
        battle::turn::phase_reset(game, Side::Enemy);
        if r == code::QUIT || r == code::LOSE {
            break r;
        }
        if r == code::WIN {
            event::poll(game, Trigger::Victory);
            break r;
        }
        r = battle::turn::phase_terrain(game, Side::Enemy);
        if r == code::LOSE || r == code::RETREAT {
            break r;
        }
        if r == code::WIN {
            event::poll(game, Trigger::Victory);
            break r;
        }
        event::poll(game, Trigger::EnemyPhaseEnd);
        game.campaign.turn += 1;
    };
    // LAB_0040d589
    let mut r = r;
    if game.campaign.vs_mode {
        if r == code::WIN || r == code::LOSE {
            let m = game.msg(if r == code::WIN { 0x10c } else { 0x10d });
            crate::ui::message_box(game, 0xb0, 200, &m, crate::ui::Wait::Click(0));
        }
        super::stop_music(game);
        gfx::fade(game, &master, 1, 0);
        game.campaign.vs_mode = false;
        return Ok(code::VS_DONE);
    }
    if r == code::LOADED || r == code::LOADED_SLOT50 {
        return Ok(r);
    }
    if r == code::LOSE {
        // FUN_0041a765: the defeat effect (FUN_0046c8cb), then the music stops.
        view::motion::stage_end(game, false);
        super::stop_music(game);
    }
    event::poll(game, Trigger::FlagChain { unit_context: false });
    if r == code::WIN {
        // FUN_0046c92c: the victory effect.
        view::motion::stage_end(game, true);
        battle::turn::victory_rewards(game);
        battle::turn::victory_exp(game);
        let c = &game.campaign;
        if c.cave_found && c.alt_next_stage > 0 {
            gfx::present_page(game, 1);
            let m = game.msg(0x35);
            if crate::ui::yes_no(game, 0xa0, 0x50, true, &m, true, false) {
                r = code::CAVE;
            }
        } else if c.alt_next_stage == c.next_stage && c.next_stage != 0 {
            r = code::CAVE;
        }
    }
    super::stop_music(game);
    if r != code::QUIT && r != code::CAVE {
        gfx::fade(game, &master, 1, 0);
    }
    if r == code::WIN {
        event::after_victory_media(game);
    }
    event::clear_stage_flags(game);
    if r != code::WIN {
        return Ok(r);
    }
    Ok(if game.campaign.next_stage != 0 { code::WIN } else { code::CAMPAIGN_DONE })
}

/// FUN_00431154: the turn-1 autosave (slot 50), not in VS.
fn autosave_turn1(game: &mut Game) -> Result<()> {
    if !game.campaign.vs_mode {
        super::init::scan_saves(game);
        if game.campaign.turn == 1 {
            super::save::write_save(game, super::save::SLOT_TURN1 + 1)?;
        }
    }
    Ok(())
}

/// FUN_0043117c: the every-turn autosave (slot 51), not in VS.
fn autosave_turn(game: &mut Game) -> Result<()> {
    if !game.campaign.vs_mode {
        super::init::scan_saves(game);
        super::save::write_save(game, super::save::SLOT_TURN + 1)?;
    }
    Ok(())
}

/// Port of `FUN_0040fa1d`: the alternate (cave) stage — the members on the
/// map stay deployed (`FUN_0044414b(player, 1)`) and take its turn-1 cells
/// (`FUN_0045cd5b`); this turn's enemies and the other player records are created.
pub fn cave_stage_setup(game: &mut Game) -> Result<()> {
    reset(game);
    super::armies::stage_end_cleanup(game, true);
    game.battle.clear_army(Side::Enemy);
    let alt = game.campaign.alt_next_stage;
    game.campaign.turn = 1;
    load_stage(game, alt, 10)?;
    map_init(game)?;
    // FUN_0046ec6b(scr, 1, 0): page 1 -> page 0 (verified in the disassembly at 0x40fa83).
    game.scr.copy_page(0, 1);
    super::narration::narration(game, 0);
    let turn = game.campaign.turn as i32;
    let appears = |game: &Game, i: usize| {
        let d = &game.battle.defs[i];
        let from = d.p.b33 as i8 as i32;
        if d.appear_to == 0 { from == turn } else { from <= turn && turn <= d.appear_to as i32 }
    };
    let (es, en, np) = (game.battle.enemy_defs_start, game.battle.n_enemy_defs, game.battle.n_player_defs);
    let enemies: Vec<usize> = (es..es + en).filter(|&i| appears(game, i)).collect();
    if !enemies.is_empty() {
        game.battle.create_units(Side::Enemy, &enemies);
    }
    super::armies::place_in_cave(game);
    let others: Vec<usize> = (0..np).filter(|&i| appears(game, i) && game.battle.defs[i].p.control != 0).collect();
    if !others.is_empty() {
        game.battle.create_units(Side::Player, &others);
    }
    view::bake(game);
    Ok(())
}
