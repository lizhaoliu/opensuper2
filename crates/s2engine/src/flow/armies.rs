//! Army bookkeeping the stage transitions do (FLOW calls them; the data is
//! BATTLE's, reached through its public fields): the end-of-stage cleanup
//! `FUN_0044414b`, the cave placement `FUN_0045cd5b` and the record split of
//! `FUN_00423033` for stages entered from a save.

use s2data::tbl_stage::Stage as BattleStage;

use crate::battle::unit::flag;
use crate::battle::{view, Placement, Side, UnitRef};
use crate::game::Game;

/// Port of `FUN_0044414b(player, keep_deployed)`: after a stage. Summon
/// slots go idle; NPC members leave unless they wait in reserve (alive);
/// summoned creatures stay only while a slot holds their kind; the others
/// face front again, a dead member with item ability 0x18 revives, and
/// everyone gets the stage reset (`FUN_004585cc`, which also takes them off
/// the map) — with `keep_deployed`, members that were on the map (not dead,
/// not in reserve, not summons) stay deployed. Leavers are removed and the
/// slots renumbered (the portrait and sprite tables are the view's).
pub fn stage_end_cleanup(game: &mut Game, keep_deployed: bool) {
    let side = Side::Player;
    let slot_kinds: Vec<i16> = game.battle.army(side).summons.iter().map(|s| s.kind).collect();
    for s in game.battle.army_mut(side).summons.iter_mut() {
        s.active = 0;
    }
    let n = game.battle.army(side).units.len();
    let mut kept = 0;
    for i in 0..n {
        let r = UnitRef::new(side, i);
        let u = game.battle.unit(r).clone();
        let npc = u.npc == 1;
        #[allow(clippy::nonminimal_bool)] // FUN_0044414b's two tests as written
        let passes = (!npc || u.flags & flag::DEAD == 0) && (!npc || u.flags & flag::RESERVE != 0);
        let keep = if !passes {
            0
        } else if u.flags & flag::SUMMON == 0 {
            1
        } else if slot_kinds.contains(&u.kind) {
            2
        } else {
            0
        };
        match keep {
            1 => {
                kept += 1;
                if u.facing != 0 {
                    game.battle.unit_mut(r).facing = 0;
                    view::set_anim(game, r, 0);
                }
                if u.flags & flag::DEAD != 0 && game.battle.has_item_ability(&u, 0x18) {
                    game.battle.unit_mut(r).flags &= !flag::DEAD;
                }
            }
            2 => kept += 1,
            _ => game.battle.unit_mut(r).kind = 0,
        }
        let f = game.battle.unit(r).flags;
        let stays = keep_deployed && f & (flag::DEAD | flag::RESERVE) == 0 && f & flag::DEPLOYED != 0;
        let u = game.battle.unit_mut(r);
        u.reset_for_stage();
        if stays && u.flags & flag::SUMMON == 0 {
            u.flags |= flag::DEPLOYED;
        }
    }
    if kept != n {
        let a = game.battle.army_mut(side);
        a.units.retain(|u| u.kind != 0);
        for (k, u) in a.units.iter_mut().enumerate() {
            u.slot = k as i16 + 1;
        }
        // FUN_0043fe07 / FUN_0043fa24(army, 0): the per-unit sprite and portrait tables are rebuilt.
        game.battle.view.units[side.index()].clear();
        game.battle.assign_portraits(side, 0);
    }
}

/// Port of `FUN_0045cd5b(player, records, n_player)`: in the cave, the
/// members who were on the map take the stage's turn-1 player cells
/// (records with first turn 1 and control 0) in order, facing as the record
/// says; members without a cell, and summons, leave the map.
pub fn place_in_cave(game: &mut Game) {
    let side = Side::Player;
    let n = game.battle.n_player_defs;
    let mut k = 0;
    for i in 0..game.battle.army(side).units.len() {
        let r = UnitRef::new(side, i);
        let f = game.battle.unit(r).flags;
        if f & flag::DEPLOYED == 0 || f & flag::DEAD != 0 {
            continue;
        }
        if f & flag::SUMMON != 0 {
            game.battle.unit_mut(r).flags &= !flag::DEPLOYED;
            continue;
        }
        while k < n && !(game.battle.defs[k].p.b33 == 1 && game.battle.defs[k].p.control == 0) {
            k += 1;
        }
        if k < n {
            let d = game.battle.defs[k].p.clone();
            if game.battle.unit(r).facing != d.facing {
                view::step_unit(game, r, d.facing);
            }
            let u = game.battle.unit_mut(r);
            u.x = d.x as i32;
            u.y = d.y as i32;
            k += 1;
        } else {
            game.battle.unit_mut(r).flags &= !flag::DEPLOYED;
        }
    }
}

/// The record split of `FUN_00423033(stage, mode)` (every mode): the player
/// prefix runs up to the first record of control 2, 3 or 8. Used when a
/// stage is entered from a save, where the armies come from the save.
pub fn set_records(game: &mut Game, stage: &BattleStage) {
    let b = &mut game.battle;
    b.defs = stage
        .units
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let base = stage.header.units_offset as usize + 101 * i;
            let mut name_raw = [0u8; 13];
            if let Some(src) = stage.block.get(base + 2..base + 15) {
                name_raw.copy_from_slice(src);
            }
            Placement { p: p.clone(), appear_to: stage.block.get(base + 0x34).map_or(0, |&v| v as i8), name_raw }
        })
        .collect();
    let n = b.defs.iter().position(|d| matches!(d.p.control, 2 | 3 | 8)).unwrap_or(b.defs.len());
    b.n_player_defs = n;
    b.n_enemy_defs = b.defs.len() - n;
    b.enemy_defs_start = n;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::battle::army::SummonSlot;

    fn data() -> Option<crate::Data> {
        let dir = std::env::var_os("S2_GAME_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("games/super2sp/extracted/app")))?;
        crate::Data::open(&dir).ok()
    }

    #[test]
    fn stage_end_cleanup_follows_the_original() {
        let Some(data) = data() else { return eprintln!("armies test: no game data, skipped") };
        let mut sys = crate::sys::Sys::new_headless(crate::sys::Shared::new(), Box::new(|_, _| {}));
        let mut game = Game::new(&mut sys, &data);
        game.res = Some(crate::res::Res::load(&data, &game.cfg).unwrap());
        game.battle.load_tables(&data).unwrap();
        crate::flow::stage::load_stage(&mut game, 1, 10).unwrap();
        game.battle.deploy_turn(1);
        let n = game.battle.army(Side::Player).units.len();
        assert!(n >= 3, "stage 1 has a party");
        {
            let a = game.battle.army_mut(Side::Player);
            a.summons[0] = SummonSlot { active: 1, kind: 0x75 };
            // 0: an NPC ally on the map leaves; 1: an NPC waiting in reserve stays;
            // 2: a wounded member is healed and taken off the map.
            a.units[0].npc = 1;
            a.units[0].flags = flag::DEPLOYED;
            a.units[1].npc = 1;
            a.units[1].flags = flag::RESERVE;
            a.units[2].npc = 0;
            a.units[2].flags = flag::DEPLOYED;
            a.units[2].hp = 1;
        }
        let keep_id = game.battle.army(Side::Player).units[1].char_id;
        stage_end_cleanup(&mut game, false);
        let a = game.battle.army(Side::Player);
        assert_eq!(a.units.len(), n - 1, "the NPC on the map left");
        assert_eq!(a.units[0].char_id, keep_id);
        assert!(a.units.iter().enumerate().all(|(k, u)| u.slot == k as i16 + 1), "slots renumbered");
        let healed = &a.units[1];
        assert_eq!(healed.hp, healed.hp_max);
        assert_eq!(healed.flags & flag::DEPLOYED, 0, "reset takes everyone off the map");
        assert_eq!(a.summons[0].active, 0, "summon slots go idle");
        assert_eq!(a.summons[0].kind, 0x75, "the slot keeps its kind");
    }

    #[test]
    fn cave_keeps_deployed_members_on_their_cells() {
        let Some(data) = data() else { return eprintln!("armies test: no game data, skipped") };
        let mut sys = crate::sys::Sys::new_headless(crate::sys::Shared::new(), Box::new(|_, _| {}));
        let mut game = Game::new(&mut sys, &data);
        game.res = Some(crate::res::Res::load(&data, &game.cfg).unwrap());
        game.battle.load_tables(&data).unwrap();
        crate::flow::stage::load_stage(&mut game, 1, 10).unwrap();
        game.battle.deploy_turn(1);
        for u in game.battle.army_mut(Side::Player).units.iter_mut() {
            u.npc = 0;
            u.flags = flag::DEPLOYED;
        }
        stage_end_cleanup(&mut game, true);
        assert!(game.battle.army(Side::Player).units.iter().all(|u| u.flags & flag::DEPLOYED != 0), "keep_deployed");
        place_in_cave(&mut game);
        let cells: Vec<(i32, i32)> = (0..game.battle.n_player_defs)
            .filter(|&k| game.battle.defs[k].p.b33 == 1 && game.battle.defs[k].p.control == 0)
            .map(|k| (game.battle.defs[k].p.x as i32, game.battle.defs[k].p.y as i32))
            .collect();
        let placed: Vec<(i32, i32)> = game.battle.army(Side::Player).units.iter().filter(|u| u.flags & flag::DEPLOYED != 0).map(|u| (u.x, u.y)).collect();
        assert_eq!(placed.len(), cells.len().min(game.battle.army(Side::Player).units.len()));
        assert_eq!(&cells[..placed.len()], &placed[..], "cells taken in order");
    }
}
