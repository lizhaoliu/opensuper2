//! Battle tests against the user's game data (`S2_GAME_DIR`, default
//! ~/games/super2sp/extracted/app). Each test is skipped when the data is absent.

use std::path::PathBuf;
use std::rc::Rc;

use s2data::tables::RuleTables;
use s2data::tbl_stage::{Stage, StageFile};

use super::army::{Side, UnitRef};
use super::map::Map;
use super::rng::{Rng, Seeder};
use super::unit::flag;
use super::Battle;

fn data() -> Option<crate::Data> {
    let dir = std::env::var_os("S2_GAME_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("games/super2sp/extracted/app")))?;
    crate::Data::open(&dir).ok()
}

thread_local! {
    static TABLES: std::cell::RefCell<Option<Rc<RuleTables>>> = const { std::cell::RefCell::new(None) };
}

/// A battle set up for campaign stage `id` at turn 1, with a fixed RNG.
fn stage_battle(id: usize) -> Option<(Battle, Stage)> {
    let d = data()?;
    let mut b = Battle::default();
    let t = TABLES.with(|c| {
        c.borrow_mut()
            .get_or_insert_with(|| Rc::new(RuleTables::load(&d.dir, &d.arcs).expect("tables")))
            .clone()
    });
    b.set_tables(t);
    b.rng = Rng::started(Seeder::fixed(1234));
    let sf = StageFile::parse(d.arcs.read(s2data::Kind::Bin, 1).ok()?).ok()?;
    let st = sf.stage(id).ok()?;
    b.set_map(Map::load(&d.arcs, st.header.map as usize).expect("map"));
    b.start_stage(&st);
    b.deploy_turn(1);
    Some((b, st))
}

#[test]
fn stage1_turn1_deployment() {
    let Some((b, st)) = stage_battle(1) else {
        eprintln!("battle tests: no game data, skipped");
        return;
    };
    eprintln!("stage 1: {} player, {} enemy units deployed", b.army(Side::Player).units.len(), b.army(Side::Enemy).units.len());
    let t = b.t();
    // Records 0..n_player_defs are player side; those appearing on turn 1 are deployed.
    assert_eq!(b.n_player_defs, 6);
    let p = b.army(Side::Player);
    let ids: Vec<i16> = p.units.iter().map(|u| u.char_id).collect();
    assert_eq!(ids, vec![11, 14, 15, 13, 12], "turn-1 player characters in record order");
    for (k, u) in p.units.iter().enumerate() {
        let d = &st.units[k];
        assert_eq!((u.x, u.y), (d.x as i32, d.y as i32), "{} cell", u.name_str());
        assert_eq!(u.slot as usize, k + 1);
        assert_eq!(u.npc, 0);
        assert!(u.on_map());
        assert_eq!(u.hp, d.stats[0]);
        assert_eq!(u.hp_max, d.stats[0]);
        assert_eq!(u.react, d.stats[4]);
        // ATK = record ATK + every equipped item's attack
        let eq_atk: i16 = u.equip.iter().filter(|&&i| i != 0).map(|&i| t.items[i as usize - 1].attack).sum();
        let eq_def: i16 = u.equip.iter().filter(|&&i| i != 0).map(|&i| t.items[i as usize - 1].defense as i16).sum();
        assert_eq!(u.atk, d.stats[5] + eq_atk);
        assert_eq!(u.atk_cur, u.atk);
        assert_eq!(u.def, d.stats[6] + eq_def);
        assert_eq!(u.eq_atk, eq_atk);
        let class_ap = t.kinds.class(d.class as u16).unwrap().base_ap as i16;
        let eq_ap: i16 = u.equip.iter().filter(|&&i| i != 0).map(|&i| t.items[i as usize - 1].ap as i16).sum();
        let ap = if d.ap < 1 { class_ap } else { d.ap };
        assert_eq!(u.ap, ap + eq_ap);
        assert_eq!(u.exp_next, t.kinds.class(d.class as u16).unwrap().exp_per_level);
        assert_eq!(u.learned.map(|v| v as i16), d.spells);
    }
    // 志超 carries the 青霜劍 as his weapon.
    let zc = &p.units[0];
    assert_eq!(t.items[zc.equip[0] as usize - 1].name, "青霜劍");
    // Enemies: records after the player prefix whose window contains turn 1.
    let expect: Vec<usize> = (b.enemy_defs_start..b.defs.len())
        .filter(|&i| b.defs[i].p.b33 == 1 && b.defs[i].appear_to == 0)
        .collect();
    let e = b.army(Side::Enemy);
    assert_eq!(e.units.len(), expect.len());
    for (u, &i) in e.units.iter().zip(&expect) {
        let d = &b.defs[i].p;
        assert_eq!(u.kind, d.class);
        assert_eq!(u.npc, 1);
        assert_eq!((u.x, u.y), (d.x as i32, d.y as i32));
        assert_eq!(u.ai_mode, d.ai_mode as i16);
    }
    // the bandit chief (control 8) is a sub-boss
    let chief = e.units.iter().find(|u| u.char_id == 64).expect("盜賊頭子");
    assert_eq!(chief.rank, 2);
    assert_eq!(chief.ai_mode, 104);
}

#[test]
fn stage1_reinforcements_by_turn() {
    let Some((mut b, _)) = stage_battle(1) else { return };
    let n1 = b.army(Side::Enemy).units.len();
    assert!(!b.deploy_turn(2) || b.army(Side::Enemy).units.len() >= n1);
    let before = b.army(Side::Enemy).units.len();
    b.deploy_turn(4);
    let added = b.army(Side::Enemy).units.len() - before;
    let expect = (b.enemy_defs_start..b.defs.len()).filter(|&i| b.defs[i].p.b33 == 4).count();
    assert_eq!(added, expect);
    // 刑天 (char 16) joins the player side on turn 5 as an NPC ally.
    b.deploy_turn(5);
    let xt = b.army(Side::Player).units.iter().find(|u| u.char_id == 16).expect("刑天");
    assert_eq!(xt.npc, 1);
    assert!(xt.flags & flag::DEPLOYED != 0);
}

#[test]
fn unit_record_round_trips_after_deploy() {
    let Some((b, _)) = stage_battle(1) else { return };
    for u in &b.army(Side::Player).units {
        assert_eq!(&super::unit::Unit::from_bytes(&u.to_bytes()), u);
    }
}

#[test]
fn movement_range_basics() {
    let Some((mut b, _)) = stage_battle(1) else { return };
    let r = UnitRef::new(Side::Player, 0); // 志超 at (13,21)
    let u = b.unit(r).clone();
    b.build_range(r, 0);
    let t = b.t();
    let root = b.range.nodes[0];
    assert_eq!((root.x, root.y), (13, 21));
    assert_eq!(root.budget as i32, (u.ap as i32 - b.c(0xe8)).min(0x6e));
    // Hand check of the four neighbours: budget = root − terrain cost − climb·rules[0xeb].
    for dir in 0..4 {
        let (x, y) = super::map::neighbour(13, 21, dir);
        let k = b.range.find_stop(x, y, false);
        let own = b.army(Side::Player).unit_at(x, y) != 0;
        let node = (1..b.range.count).map(|i| b.range.order[i] as usize).find(|&i| {
            let n = b.range.nodes[i];
            (n.x as i32, n.y as i32) == (x, y)
        });
        let (pass, eff) = b.map().terrain(&t, x, y, u.kind, 0);
        if pass == super::map::Pass::Blocked {
            assert!(node.is_none());
            continue;
        }
        let n = b.range.nodes[node.expect("neighbour reachable")];
        let dh = root.height as i32 - b.map().height(x, y);
        let climb = if dh < 1 { -dh } else { 0 };
        let expect = root.budget as i32 - eff.cost as i32 - climb * b.c(0xeb);
        // a neighbour may also be reached more cheaply through a detour only if costs differ; at
        // distance 1 the direct step is never worse for uniform terrain, so check ≥
        assert!(n.budget as i32 >= expect, "dir {dir}: {} < {expect}", n.budget);
        assert_eq!(n.can_stop == 1, !own && pass == super::map::Pass::Ok, "dir {dir} stop flag");
        assert_eq!(k != 0, n.can_stop == 1);
    }
    // Every node's budget is the best of its in-range neighbours' steps (no
    // cheaper path was missed) and parents are edge-adjacent.
    for i in 1..b.range.count {
        // FUN_00472403's compaction can copy the root index 0 into the order
        // list (it zeroes a deleted tail entry, then tests nodes[0]); skip it.
        if b.range.order[i] == 0 {
            continue;
        }
        let n = b.range.nodes[b.range.order[i] as usize];
        let p = b.range.nodes[n.parent as usize];
        assert_eq!(
            super::map::distance(p.x as i32, p.y as i32, n.x as i32, n.y as i32),
            1,
            "node {} {:?} parent {} {:?}",
            b.range.order[i],
            n,
            n.parent,
            p
        );
        assert!(n.budget >= 0 && n.budget <= root.budget);
    }
    // No enemy cell is a stop cell, and the path to a far stop cell walks back to the root.
    let (far, _) = b.range.reachable().filter(|(_, n)| n.can_stop == 1).max_by_key(|(_, n)| -(n.budget as i32)).unwrap();
    b.range.build_path(far + 1, false);
    assert_eq!(*b.range.path.last().unwrap(), 0);
    let steps = b.walk_steps();
    assert_eq!(steps.len(), b.range.path.len() - 1);
    let fd = b.range.nodes[far];
    assert_eq!((steps.last().unwrap().0, steps.last().unwrap().1), (fd.x as i32, fd.y as i32));
    let ap_left = fd.budget;
    b.execute_move(r);
    assert_eq!((b.unit(r).x, b.unit(r).y), (fd.x as i32, fd.y as i32));
    assert_eq!(b.unit(r).ap, ap_left);
}

/// Put enemy unit `e` next to `r` facing it, so the duel is frontal.
fn face_off(b: &mut Battle, r: UnitRef, e: UnitRef) -> (i32, i32) {
    let (x, y) = (b.unit(r).x, b.unit(r).y);
    let t = b.t();
    let ek = b.unit(e).kind;
    let cell = (0..4)
        .map(|d| super::map::neighbour(x, y, d))
        .find(|&(nx, ny)| {
            b.map().terrain(&t, nx, ny, ek, 0).0 == super::map::Pass::Ok
                && b.army(Side::Player).unit_at(nx, ny) == 0
                && b.army(Side::Enemy).unit_at(nx, ny) == 0
        })
        .expect("free neighbour");
    let f = super::movement::facing_toward(cell.0, cell.1, x, y, b.unit(e).facing);
    let u = b.unit_mut(e);
    (u.x, u.y) = cell;
    super::movement::set_facing(u, f);
    cell
}

#[test]
fn scripted_attack_matches_formula() {
    let Some((mut b, _)) = stage_battle(1) else { return };
    let t = b.t();
    let ctx = super::rules::Ctx::new(&t, b.difficulty, b.vs);
    let p = |i| ctx.p(i);
    let att = UnitRef::new(Side::Player, 0); // 志超
    let def = UnitRef::new(Side::Enemy, 0);
    face_off(&mut b, att, def);
    let skill = t.kinds.class(b.unit(att).kind as u16).unwrap().actions.start;
    assert!(b.can_attack_with(att, skill));
    let d = b.duel(att, def, skill);
    assert_eq!(d.facing, 1, "frontal");
    let (a, v) = (b.unit(att).clone(), b.unit(def).clone());
    let (fa, fv) = (b.fighter(att), b.fighter(def));
    let dh = if b.difficulty == 1 { 0 } else { fa.height - fv.height };
    let ta = super::rules::terrain_for(b.difficulty, fa.terrain);
    let tv = super::rules::terrain_for(b.difficulty, fv.terrain);
    // weapon proficiency: the unit's prof slot for its weapon kind
    let wkind = t.items[a.equip[0] as usize - 1].kind as i32;
    let prof = a.prof[(wkind - 1) as usize] as i32;
    assert_eq!(d.calc.att_prof, prof);
    // forecast hit before modifiers, as docs/battle.md "Forecast" gives it
    let raw_hit = a.skill_cur as i32 * (dh * 1000 + 8000 + prof) / 8000 - v.react as i32;
    let hit = super::rules::hit_mods(&ctx, &fa, &fv, skill, raw_hit);
    assert_eq!(d.calc.fc_hit, (hit * (p(2) + (p(3) + 3) / 2) / 10).clamp(1, 99));

    // Replay the strike's RNG draws by hand (both at full HP: no desperation draws).
    let snap = b.rng.snapshot();
    let mut dd = d;
    let got = b.roll_attack(&mut dd);
    b.rng.restore(snap);
    let r = &mut b.rng;
    let hit = (hit * 12 / 10 * (p(2) + r.rnd(p(3) + 3)) / 10).clamp(1, 99);
    let expect = if hit < r.pct() {
        0
    } else {
        let div = super::rules::random_divisor(r);
        let cl = dh.clamp(-8, 4);
        let k = if cl < 0 { 125 } else { 250 };
        let base = (a.atk_cur as i32 - v.def_cur as i32)
            * (cl * k + 1000 + (ta[1] as i32 - tv[2] as i32) * 10 + prof)
            / 10
            / div;
        let mut dmg = super::rules::damage_mods(&ctx, base, 0, &fa, &fv, skill);
        if dh > 0 {
            dmg += dh;
        }
        dmg = (dmg * (p(0) + r.rnd(p(1) + 3)) / 10).max(1);
        if dmg < 3 {
            let s = r.pct();
            dmg = if s < 1 { 5 } else if s < 6 { 4 } else if s < 16 { 3 } else if s < 46 { 2 } else { 1 };
        }
        dmg
    };
    assert_eq!(got, expect);
    eprintln!("志超 → {}: forecast hit {} dmg {}, rolled {}", v.name_str(), d.calc.fc_hit, d.calc.fc_dmg, got);

    // Applying it lowers HP by exactly the damage, and lethal damage kills.
    let hp = b.unit(def).hp as i32;
    b.apply_hp(att, def, got, 1);
    assert_eq!(b.unit(def).hp as i32, (hp - got).max(0));
    b.apply_hp(att, def, 9999, 1);
    assert_eq!(b.unit(def).hp, 0);
}

#[test]
fn same_seed_same_battle() {
    let run = || {
        let (mut b, _) = stage_battle(1)?;
        let att = UnitRef::new(Side::Player, 0);
        let def = UnitRef::new(Side::Enemy, 0);
        face_off(&mut b, att, def);
        let skill = b.t().kinds.class(b.unit(att).kind as u16).unwrap().actions.start;
        let mut out = Vec::new();
        for _ in 0..20 {
            let mut d = b.duel(att, def, skill);
            out.push((b.roll_attack(&mut d), b.roll_counter(&d), b.roll_crit(&d)));
        }
        Some(out)
    };
    let (Some(x), Some(y)) = (run(), run()) else { return };
    assert_eq!(x, y);
    assert!(x.iter().any(|v| v.0 > 0), "some hits in 20 tries");
}

/// Stage 1 on a headless `Game`: every enemy is attacked by 志超 (moved next
/// to it each time) until it falls, through the real attack sequence with
/// its script hooks; the stage check then reports the victory and the
/// rewards (刑天 joins, 1500 gold, items) are applied.
#[test]
fn stage1_fought_to_victory() {
    use crate::audio::Audio;
    use crate::event;
    use crate::game::Game;
    use crate::sys::{Shared, Sys};
    let Some(d) = data() else { return };
    let scripts = s2data::script::Stage::parse_all(&d.arcs.read(s2data::Kind::Bin, s2data::script::BIN_STAGES).unwrap()).unwrap();
    let mut sys = Sys::new(Shared::new(), Audio::silent());
    let mut game = Game::new(&mut sys, &d);
    game.events.ui = event::ui::UiHooks::headless();
    game.battle.load_tables(&d).unwrap();
    game.battle.rng = Rng::started(Seeder::fixed(7));
    let sf = StageFile::parse(d.arcs.read(s2data::Kind::Bin, 1).unwrap()).unwrap();
    let st = sf.stage(1).unwrap();
    game.battle.set_map(Map::load(&d.arcs, st.header.map as usize).unwrap());
    game.battle.start_stage(&st);
    event::attach_stage(&mut game, &scripts[0], true);
    game.stage = Some(crate::flow::stage::StageState { stage: scripts[0].clone(), battle_stage: st });
    game.campaign.turn = 1;
    super::turn::deploy_turn_units(&mut game);
    assert_eq!(super::turn::stage_check(&mut game), 0);
    let att = UnitRef::new(Side::Player, 0);
    let mut swings = 0;
    for turn in 1..=6 {
        game.campaign.turn = turn;
        super::turn::deploy_turn_units(&mut game);
        loop {
            let alive: Vec<usize> = (0..game.battle.army(Side::Enemy).units.len())
                .filter(|&i| {
                    let u = &game.battle.army(Side::Enemy).units[i];
                    u.on_map() && !u.is_dead()
                })
                .collect();
            let Some(&e) = alive.first() else { break };
            let def = UnitRef::new(Side::Enemy, e);
            face_off(&mut game.battle, att, def);
            let skill = game.battle.t().kinds.class(game.battle.unit(att).kind as u16).unwrap().actions.start;
            let tl = game.battle.target_list(att, skill);
            let ti = tl.target_at(game.battle.unit(def).x, game.battle.unit(def).y);
            assert!(ti > 0, "target in range");
            let duel = game.battle.duel(att, def, skill);
            super::actions::attack(&mut game, &tl, ti - 1, duel);
            // Keep the hero standing for the test.
            let u = game.battle.unit_mut(att);
            u.hp = u.hp_max;
            swings += 1;
            assert!(swings < 400, "the enemies keep standing");
        }
    }
    assert!(game.battle.unit(att).exp > 0 || game.battle.unit(att).level > 1, "EXP was awarded");
    assert_eq!(super::turn::stage_check(&mut game), 1, "all enemies down = victory");
    let gold = game.battle.army(Side::Player).gold;
    super::turn::victory_rewards(&mut game);
    assert_eq!(game.battle.army(Side::Player).gold, gold + 1500);
    let xt = game.battle.army(Side::Player).units.iter().find(|u| u.char_id == 16);
    assert!(xt.is_some_and(|u| u.npc == 0), "刑天 joined");
    eprintln!("stage 1 won after {swings} attacks; 志超 level {} exp {}", game.battle.unit(att).level, game.battle.unit(att).exp);
}

/// FUN_00466f65 kind 6 (spell 109, a combo): on a headless `Game` the cast
/// sequence applies the cells itself, four at a time after the first; the
/// first cell is not hit and the EXP is capped at 0x3c.
#[test]
fn kind6_sequence_skips_the_first_cell() {
    use crate::audio::Audio;
    use crate::event;
    use crate::game::Game;
    use crate::sys::{Shared, Sys};
    use super::spells::Cast;
    let Some(d) = data() else { return };
    let scripts = s2data::script::Stage::parse_all(&d.arcs.read(s2data::Kind::Bin, s2data::script::BIN_STAGES).unwrap()).unwrap();
    let mut sys = Sys::new(Shared::new(), Audio::silent());
    let mut game = Game::new(&mut sys, &d);
    game.events.ui = event::ui::UiHooks::headless();
    game.battle.load_tables(&d).unwrap();
    let sf = StageFile::parse(d.arcs.read(s2data::Kind::Bin, 1).unwrap()).unwrap();
    let st = sf.stage(1).unwrap();
    game.battle.set_map(Map::load(&d.arcs, st.header.map as usize).unwrap());
    game.battle.start_stage(&st);
    event::attach_stage(&mut game, &scripts[0], true);
    game.campaign.turn = 1;
    super::turn::deploy_turn_units(&mut game);
    let spell = 109u16;
    assert_eq!(super::view::cast::spell_fx(&game, spell).kind, 6);
    let (r, p) = (UnitRef::new(Side::Player, 0), UnitRef::new(Side::Player, 1));
    let (cx, cy) = (game.battle.unit(r).x, game.battle.unit(r).y);
    let aim = game.battle.spell_aim(spell as i32);
    let mut seen_hit = false;
    for seed in 0..40 {
        game.battle.rng = Rng::started(Seeder::fixed(seed));
        let mut tg = game.battle.spell_target(Side::Player, r, spell as i32, 0, 2, false);
        game.battle.target_range(&mut tg, aim.range);
        let Some(&(tx, ty)) = tg.range.iter().find(|&&c| c != (cx, cy)) else { return };
        game.battle.aim_ai_cast(&mut tg, tx, ty, 0);
        let cells = tg.cells().to_vec();
        if cells.len() < 2 {
            return;
        }
        // Enemies on the first two cells (off the map otherwise).
        let foes: Vec<UnitRef> = (0..2).map(|k| UnitRef::new(Side::Enemy, k)).collect();
        for (k, &f) in foes.iter().enumerate() {
            let u = game.battle.unit_mut(f);
            (u.x, u.y) = cells[k];
            u.hp = u.hp_max;
            u.flags &= !flag::DEAD;
        }
        let hp0: Vec<i16> = foes.iter().map(|&f| game.battle.unit(f).hp).collect();
        let mut c = Cast::new(Side::Player, r, spell, (tx, ty));
        c.partner = Some(p);
        let out = super::cast::cast_effects(&mut game, &mut c, &mut tg);
        assert_eq!(game.battle.unit(foes[0]).hp, hp0[0], "kind 6 does not hit the first cell");
        assert!(out.targets.iter().all(|h| h.unit != foes[0]));
        assert!(c.exp <= 0x3c && c.exp_partner == c.exp);
        if game.battle.unit(foes[1]).hp < hp0[1] {
            seen_hit = true;
            break;
        }
    }
    assert!(seen_hit, "the second cell is hit by some seed");
}

/// The cast presentation on a presenting `Game` (frames captured; PNGs in
/// `S2_BATTLE_DUMP` when set): spell 98 on an adjacent enemy through
/// `cast::cast_effects` (animation, hit flash, FUN_004636cd's popup with
/// the HP counted, death or not), then the kind 6 spell 109 cell by cell.
#[test]
fn cast_presentation_frames() {
    use std::sync::{Arc, Mutex};
    use crate::game::Game;
    use crate::sys::{FlipInfo, Shared, Sys};
    use super::spells::Cast;
    let Some(d) = data() else { return };
    let dump = std::env::var_os("S2_BATTLE_DUMP").map(PathBuf::from);
    let frames: Arc<Mutex<Vec<Vec<u8>>>> = Arc::default();
    let f2 = frames.clone();
    let hook = Box::new(move |info: &FlipInfo, _: &Shared| {
        let mut v = f2.lock().unwrap();
        if v.last().is_none_or(|l| l.as_slice() != info.pixels) {
            v.push(info.pixels.to_vec());
        }
    });
    let mut sys = Sys::new_headless(Shared::new(), hook);
    let mut game = Game::new(&mut sys, &d);
    let res = crate::res::Res::load(&d, &game.cfg).unwrap();
    let pal = s2data::gfx_palette::vga_to_rgb(&res.master);
    game.res = Some(res);
    game.scr.hw = pal;
    game.battle.load_tables(&d).unwrap();
    game.battle.rng = Rng::started(Seeder::fixed(3));
    let st = StageFile::parse(d.arcs.read(s2data::Kind::Bin, 1).unwrap()).unwrap().stage(1).unwrap();
    game.battle.set_map(Map::load(&d.arcs, st.header.map as usize).unwrap());
    game.battle.start_stage(&st);
    game.battle.deploy_turn(1);
    game.battle.turn = 1;
    super::view::init(&mut game, st.header.map as usize).unwrap();
    let save = |name: &str, px: &[u8]| {
        let Some(dir) = &dump else { return };
        std::fs::create_dir_all(dir).unwrap();
        let f = std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{name}.png"))).unwrap());
        let mut enc = png::Encoder::new(f, 640, 480);
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_palette(pal.iter().flatten().copied().collect::<Vec<u8>>());
        enc.write_header().unwrap().write_image_data(px).unwrap();
    };
    let (r, p) = (UnitRef::new(Side::Player, 0), UnitRef::new(Side::Player, 1));
    let (px, py) = (game.battle.unit(r).x, game.battle.unit(r).y);
    let e0 = UnitRef::new(Side::Enemy, 0);
    let (ex, ey) = super::map::neighbour(px, py, 1);
    {
        let u = game.battle.unit_mut(e0);
        (u.x, u.y) = (ex, ey);
    }
    game.battle.map_mut().center_on(px, py, true);
    super::view::bake(&mut game);
    for (spell, partner) in [(98u16, None), (109, Some(p))] {
        let aim = game.battle.spell_aim(spell as i32);
        let mut tg = game.battle.spell_target(Side::Player, r, spell as i32, 0, 2, false);
        game.battle.target_range(&mut tg, aim.range);
        game.battle.aim_ai_cast(&mut tg, ex, ey, 1);
        let mut c = Cast::new(Side::Player, r, spell, (ex, ey));
        c.partner = partner;
        let hp0 = game.battle.unit(e0).hp;
        let n0 = frames.lock().unwrap().len();
        let out = super::cast::cast_effects(&mut game, &mut c, &mut tg);
        let fr = frames.lock().unwrap()[n0..].to_vec();
        eprintln!("spell {spell}: {} frames, hp {hp0} -> {}, hits {:?}", fr.len(), game.battle.unit(e0).hp, out.targets);
        assert!(fr.len() > 8, "spell {spell} presented {} frames", fr.len());
        let step = (fr.len() / 12).max(1);
        for (k, f) in fr.iter().enumerate().step_by(step) {
            save(&format!("cast{spell}_{k:03}"), f);
        }
        let u = game.battle.unit_mut(e0);
        u.hp = u.hp_max;
        u.flags &= !flag::DEAD;
    }
}

/// Regression: `Map::object_class` is already `attr >> 8`. Stage 16's exit
/// (map 16, the script header's map; the battle-stage header byte is not
/// the map) at (1, 19) is object class 6, so FUN_0045cbf8 (win codes 0x1f /
/// 0x20) holds once 志超 (route 0) or any deployed own unit stands there.
#[test]
fn stage16_exit_tile_is_reached() {
    use crate::audio::Audio;
    use crate::game::Game;
    use crate::sys::{Shared, Sys};
    let Some(d) = data() else { return };
    let mut sys = Sys::new(Shared::new(), Audio::silent());
    let mut game = Game::new(&mut sys, &d);
    game.battle.load_tables(&d).unwrap();
    let sf = StageFile::parse(d.arcs.read(s2data::Kind::Bin, 1).unwrap()).unwrap();
    let st = sf.stage(16).unwrap();
    game.battle.set_map(Map::load(&d.arcs, 16).unwrap());
    game.battle.start_stage(&st);
    game.battle.deploy_turn(1);
    assert_eq!(game.battle.map().object_class(1, 19), 6, "map 16 exit tile class");
    game.battle.route = 0;
    let p = &mut game.battle.army_mut(Side::Player).units;
    p.clear();
    let mut u = super::unit::Unit::default();
    u.char_id = 0xb;
    u.flags = flag::DEPLOYED;
    (u.x, u.y) = (10, 30);
    p.push(u);
    assert_ne!(game.battle.map().object_class(10, 30), 6);
    assert!(!super::hooks::on_exit_tile(&mut game, 0));
    assert!(!super::hooks::on_exit_tile(&mut game, 1));
    let u = &mut game.battle.army_mut(Side::Player).units[0];
    (u.x, u.y) = (1, 19);
    assert!(super::hooks::on_exit_tile(&mut game, 0), "志超 on the exit");
    assert!(super::hooks::on_exit_tile(&mut game, 1), "a deployed unit on the exit");
}

/// Regression: FUN_0044fae0 sets story flag 8 (`DAT_0049450f`) for a player
/// unit on an object-class-4 tile (the red flag) and 6 (`DAT_0049450d`) for
/// an enemy on class 3. Stage 50 (提那南橋) chains flag 8 to flag 4 (win).
#[test]
fn stage50_red_flag_wins() {
    use crate::audio::Audio;
    use crate::event;
    use crate::game::Game;
    use crate::sys::{Shared, Sys};
    let Some(d) = data() else { return };
    let scripts = s2data::script::Stage::parse_all(&d.arcs.read(s2data::Kind::Bin, s2data::script::BIN_STAGES).unwrap()).unwrap();
    let script = scripts.iter().find(|s| s.id == 50).unwrap().clone();
    let mut sys = Sys::new(Shared::new(), Audio::silent());
    let mut game = Game::new(&mut sys, &d);
    game.events.ui = event::ui::UiHooks::headless();
    game.battle.load_tables(&d).unwrap();
    let sf = StageFile::parse(d.arcs.read(s2data::Kind::Bin, 1).unwrap()).unwrap();
    let st = sf.stage(50).unwrap();
    game.battle.set_map(Map::load(&d.arcs, script.header.map as usize).unwrap());
    game.battle.start_stage(&st);
    event::attach_stage(&mut game, &script, true);
    game.stage = Some(crate::flow::stage::StageState { stage: script, battle_stage: st });
    game.campaign.turn = 1;
    super::turn::deploy_turn_units(&mut game);
    let m = game.battle.map();
    let find = |class: u16| (0..m.rows()).flat_map(|y| (0..m.width()).map(move |x| (x, y))).find(|&(x, y)| m.object_class(x, y) == class);
    let flag_cell = find(4).expect("a red-flag tile on stage 50's map");
    // A deployed party member (the protagonist, so the lose check stays quiet).
    let mut u = super::unit::Unit::default();
    u.char_id = 0xb;
    u.hp = 100;
    u.hp_max = 100;
    u.flags = flag::DEPLOYED;
    (u.x, u.y) = flag_cell;
    game.battle.army_mut(Side::Player).units.push(u);
    let r = UnitRef::new(Side::Player, game.battle.army(Side::Player).units.len() - 1);
    assert_eq!(game.campaign.flags[8], 0);
    super::menu::after_action(&mut game, r);
    assert_eq!(game.campaign.flags[8], 1, "flag 8 = DAT_0049450f");
    assert_eq!(game.campaign.flags[7], 0);
    assert_eq!(super::turn::stage_check(&mut game), 1, "flag 8 chains to flag 4: victory");
}

/// FUN_0044306f on a loaded save: a dead enemy extra (npc 1, no character)
/// gets no frame, so its sprite sizes stay cleared (stage 90's turn-33
/// autosave); a dead named unit gets FUN_00443c84 (facing byte 0, slot 0)
/// and keeps its frame byte.
#[test]
fn loaded_dead_units_sprite_fields() {
    let mut b = Battle::default();
    let mut extra = super::unit::Unit::default();
    extra.flags = flag::DEPLOYED | flag::DEAD;
    extra.npc = 1;
    extra.kind = 23;
    extra.sprite = 3;
    extra.sprite_w = 0x80;
    extra.sprite_h = 0xa0;
    extra.facing = 1;
    let mut named = extra.clone();
    named.char_id = 159;
    named.frame = 2;
    b.army_mut(Side::Enemy).units.push(extra);
    b.army_mut(Side::Enemy).units.push(named);
    b.slots_loaded(Side::Enemy, false);
    let u = &b.army(Side::Enemy).units[0];
    assert_eq!((u.sprite, u.sprite_w, u.sprite_h), (0, 0, 0));
    assert_eq!(u.facing, 1, "the extra is not given a frame (FUN_00443c84)");
    let u = &b.army(Side::Enemy).units[1];
    assert_eq!((u.sprite, u.facing, u.frame), (0, 0, 2));
    assert_eq!((u.sprite_w, u.sprite_h), (0x80, 0xa0));
}
