//! The cast animation of the SpellCaster object (`DAT_004a1f58`): the
//! casters' pose (FUN_00466ba5), the effect scene with its tint and lit
//! targets (FUN_004663a8 / FUN_00466194), the ripple players of the
//! sequence kinds 9/10 (FUN_004676b9 / FUN_004679ae) and the effect on the
//! aimed cell, ported from FUN_00465f44. The per-kind sequence
//! FUN_00466f65 interleaves game logic (kinds 2/3/6 apply the cast cell by
//! cell), so it is the caller's hook; [`draw_sequence`] is its drawing
//! alone. The cast's effects are applied afterwards (FUN_0045ff9e).

use s2data::tables::SpellDef;

use crate::game::Game;

use super::super::actions::presenting;
use super::super::army::UnitRef;
use super::super::camera;
use super::super::movement::facing_toward;
use super::super::spells::Cast;
use super::super::targeting::SpellTarget;
use super::unit::ACT_COMBAT;

/// The four BIN 10 presentation bytes of a spell.
#[derive(Debug, Clone, Copy, Default)]
pub struct SpellFx {
    /// `DAT_004a255c`: effect frame set − 1 (0 = no effect).
    pub effect: u8,
    /// `DAT_004a1f54`: the sequence (1 two-part with the target hidden, 4
    /// directional, 5 on the casters first, 6 four cells at a time, 7
    /// scroll and shake first, 8 shake after, 9 ripple over the units, 10
    /// ripple over the cells; 2 and 3 are unused by the data).
    pub kind: u8,
    /// `DAT_004a1f3c` / `DAT_004a1f28`: the scene is tinted toward this
    /// colour by this percentage while the effect plays.
    pub tint: u8,
    pub tint_pct: u8,
}

/// The BIN 10 presentation bytes of 1-based spell `spell`.
pub fn spell_fx(game: &Game, spell: u16) -> SpellFx {
    let t = game.battle.t();
    let at = |k: usize| t.params.per_spell[k].get((spell as usize).wrapping_sub(1)).copied().unwrap_or(0);
    SpellFx { effect: at(0), kind: at(1), tint: at(2), tint_pct: at(3) }
}

fn spell_def(game: &Game, spell: u16) -> Option<SpellDef> {
    game.battle.t().spells.spells.get((spell as usize).wrapping_sub(1)).cloned()
}

fn pos(game: &Game, r: UnitRef) -> (i32, i32) {
    let u = game.battle.unit(r);
    (u.x, u.y)
}

/// The cell the effect is aimed at: a teleport's (effect 4) first pick,
/// otherwise the cast's cell.
fn aim_cell(cast: &Cast, sp: &SpellDef, tg: &SpellTarget) -> (i32, i32) {
    if sp.effect == 4 {
        tg.target_cell
    } else {
        cast.cell
    }
}

fn combo_partner(cast: &Cast, sp: &SpellDef) -> Option<UnitRef> {
    if sp.combo & 7 != 0 {
        cast.partner
    } else {
        None
    }
}

/// Port of FUN_00465f44: the whole cast animation of `cast`, aimed by `tg`:
/// the camera to the caster, the pose, then `sequence` (FUN_00466f65:
/// gets the effect id, `DAT_004a255c[spell − 1] + 1`, which it may
/// advance, and returns true when it was the whole animation) and the
/// effect on the aimed cell (with 動畫播放 on). Leaves the caster's scene
/// in pages 0 and 1 (not presented) and the caster selected (`act` 1).
pub fn cast_animation<F: FnMut(&mut Game, &mut usize) -> bool>(game: &mut Game, cast: &Cast, tg: &SpellTarget, mut sequence: F) {
    if !presenting(game) {
        return;
    }
    let Some(sp) = spell_def(game, cast.spell) else { return };
    let r = cast.caster;
    let (x, y) = pos(game, r);
    if !game.battle.map().on_screen(x, y, 0) {
        camera::scroll_to(game, x, y, Some(r));
    }
    caster_pose(game, cast, tg);
    let sf = spell_fx(game, cast.spell);
    let mut effect = sf.effect as usize + 1;
    if sequence(game, &mut effect) || sf.effect == 0 {
        return;
    }
    let anim = game.cfg.battle_anim;
    let (tx, ty) = aim_cell(cast, &sp, tg);
    match sf.kind {
        1 => {
            // First part as a plain effect, then the next set with the
            // target taken out of the scene.
            if anim {
                effect_on(game, cast, tg, 0, effect, tx, ty);
            }
            effect += 1;
        }
        4 => effect += tg.dir,
        _ => {}
    }
    game.battle.unit_mut(r).act = 0;
    if sp.effect != 3 && sf.kind != 1 {
        camera::scroll_exact(game, tx, ty, Some(r));
    }
    if anim {
        effect_on(game, cast, tg, sf.kind, effect, tx, ty);
    }
    super::motion::draw_window(game);
    super::draw_unit(game, r, 0);
    game.scr.copy_page(1, 0);
    if sf.kind == 8 && anim {
        super::motion::shake_screen(game, 0xe, effect);
    }
    game.battle.unit_mut(r).act = 1;
}

/// Port of FUN_00466ba5: caster (and combo partner) face the aimed cell
/// and play their magic (action kind 0xb, sound 0x62) or technique (0xc,
/// sound 0x63) animation set; without one, set 2 (set 1 for an item).
pub fn caster_pose(game: &mut Game, cast: &Cast, tg: &SpellTarget) {
    let Some(sp) = spell_def(game, cast.spell) else { return };
    let sp = &sp;
    let (tx, ty) = aim_cell(cast, sp, tg);
    let technique = {
        let t = game.battle.t();
        let first_tech = t.spells.school_spells.get(t.spells.magic_schools as usize).map_or(0, |s| s.0 as i32);
        first_tech < cast.spell as i32
    };
    let kind = if technique { 0xc } else { 0xb };
    let default = if cast.from_item { 1 } else { 2 };
    let pose = |game: &Game, u: UnitRef| -> u8 {
        let t = game.battle.t();
        t.kinds
            .class(game.battle.unit(u).kind as u16)
            .and_then(|c| c.actions.clone().find(|&a| t.kinds.actions[a].kind as i8 as i32 == kind))
            .map_or(default, |a| t.kinds.actions[a].anim_set)
    };
    let face = |game: &Game, u: UnitRef| {
        let v = game.battle.unit(u);
        facing_toward(v.x, v.y, tx, ty, v.facing)
    };
    let mut acts = vec![(cast.caster, face(game, cast.caster), pose(game, cast.caster))];
    let partner = combo_partner(cast, sp);
    if let Some(p) = partner {
        // FUN_00441c0b: the partner leaves the surface (the units redrawn
        // around it include a caster that has moved, act 4).
        let f = face(game, p);
        super::erase_unit(game, p);
        acts.push((p, f, pose(game, p)));
    }
    game.sys.audio.play_sound(&game.data.arcs, if technique { 0x63 } else { 0x62 }, 0);
    super::play_actions(game, &acts);
    if let Some(p) = partner {
        super::put_unit(game, p);
    }
}

/// The drawing of FUN_00466f65 alone, a `sequence` for [`cast_animation`]
/// until the caller ports the original's: the per-kind sequences that
/// replace or precede the effect on the aimed cell. True when the
/// sequence was the whole animation (kinds 2, 3, 6, 9, 10).
///
/// The original applies kinds 2, 3 and 6 cell by cell between the effect
/// frames (FUN_00460c85: kind 2 charges the cost per cell while caster and
/// partner can pay, kind 3 skips the first cell, kind 6 hits four cells at
/// a time); here only their animation runs.
pub fn draw_sequence(game: &mut Game, cast: &Cast, tg: &SpellTarget, effect: &mut usize) -> bool {
    let Some(sp) = spell_def(game, cast.spell) else { return false };
    let sp = &sp;
    let sf = spell_fx(game, cast.spell);
    let anim = game.cfg.battle_anim;
    let cells = tg.cells().to_vec();
    match sf.kind {
        2 => {
            if anim {
                effect_scene(game, cast, tg, 2);
                let cost = sp.cost as i32;
                let mp = |game: &Game, u: Option<UnitRef>| u.map_or(i32::MAX / 2, |u| game.battle.unit(u).mp as i32);
                let (mut a, mut b) = (mp(game, Some(cast.caster)), mp(game, cast.partner));
                for &(x, y) in &cells {
                    let _ = super::play_effect(game, *effect, x, y, false);
                    a -= cost;
                    b -= cost;
                    if a < cost || b < cost {
                        break;
                    }
                }
            }
            true
        }
        3 => {
            if anim {
                effect_scene(game, cast, tg, 3);
                for &(x, y) in cells.iter().skip(1) {
                    let _ = super::play_effect(game, *effect, x, y, false);
                }
            }
            true
        }
        5 => {
            if anim {
                let mut on = vec![pos(game, cast.caster)];
                if let Some(p) = combo_partner(cast, sp) {
                    on.push(pos(game, p));
                }
                effect_scene(game, cast, tg, 5);
                let _ = super::play_effect_cells(game, *effect, &on);
            }
            *effect += 1;
            false
        }
        6 => {
            if anim {
                effect_scene(game, cast, tg, 6);
                for four in cells.get(1..).unwrap_or(&[]).chunks(4) {
                    let _ = super::play_effect_cells(game, *effect, four);
                }
            }
            true
        }
        7 => {
            let (x, y) = cast.cell;
            camera::scroll_exact(game, x, y, Some(cast.caster));
            game.scr.set_page(1);
            super::motion::draw_window(game);
            super::draw_unit(game, cast.caster, 0);
            game.scr.set_page(0);
            if anim {
                super::motion::shake_screen(game, 7, *effect);
                let snd = game.battle.view.assets().fx.sound(*effect) as u16;
                game.sys.audio.play_sound(&game.data.arcs, snd, 0);
            }
            false
        }
        9 => {
            if anim {
                ripple_units(game, cast, tg, *effect);
            }
            true
        }
        10 => {
            if anim {
                ripple_cells(game, cast, tg, *effect);
            }
            true
        }
        _ => false,
    }
}

/// Port of FUN_0046637b: the scene, then effect `effect` on (x, y)
/// (`kind` 7 keeps the units over it). Its FUN_004664ec only frees the
/// frame set (dropped here).
pub fn effect_on(game: &mut Game, cast: &Cast, tg: &SpellTarget, kind: u8, effect: usize, x: i32, y: i32) {
    effect_scene(game, cast, tg, kind);
    let _ = super::play_effect(game, effect, x, y, kind == 7);
}

/// Port of FUN_004663a8: the effect clock starts and page 1 gets the
/// effect's backdrop: the map window with the caster; for `kind` 1 without
/// the unit on the aimed cell; tinted with the casters and the targets
/// drawn untinted (FUN_00466194). Ends on page 0.
pub fn effect_scene(game: &mut Game, cast: &Cast, tg: &SpellTarget, kind: u8) {
    let Some(sp) = spell_def(game, cast.spell) else { return };
    let sp = &sp;
    let sf = spell_fx(game, cast.spell);
    game.battle.view.fx_last = game.ticks();
    let r = cast.caster;
    let gone = if kind == 1 {
        let (x, y) = aim_cell(cast, sp, tg);
        let own = game.battle.unit_at_cell(cast.side, x, y, false);
        if sp.effect == 4 {
            own.or_else(|| game.battle.unit_at_cell(cast.side.other(), x, y, false))
        } else {
            own
        }
    } else {
        None
    };
    game.scr.set_page(1);
    // FUN_00441e09 restores what lies behind that unit: draw the window
    // with it lifted off the surface instead.
    let saved = gone.map(|u| game.battle.unit(u).act);
    if let Some(u) = gone {
        game.battle.unit_mut(u).act = ACT_COMBAT;
    }
    super::motion::draw_window(game);
    if gone != Some(r) {
        super::draw_unit(game, r, 0);
    }
    if let (Some(u), Some(a)) = (gone, saved) {
        game.battle.unit_mut(u).act = a;
    }
    tint_scene(game, cast, sp, tg, sf);
    game.scr.set_page(0);
}

/// Port of FUN_00466194: page 1 tinted toward the spell's colour, the
/// casters redrawn over it, and on every target cell the unit the spell
/// affects (opponents unless it targets its own side, own units unless
/// it targets enemies).
fn tint_scene(game: &mut Game, cast: &Cast, sp: &SpellDef, tg: &SpellTarget, sf: SpellFx) {
    if sf.tint_pct == 0 || sf.tint == 0 {
        return;
    }
    game.scr.set_page(1);
    {
        let Game { battle, scr, .. } = &mut *game;
        let tree = &battle.view.assets().tree;
        // FUN_0042074e over the whole page.
        let mut cache = [0u8; 256];
        for p in scr.pages[1].pixels.iter_mut().filter(|p| **p != 0) {
            if cache[*p as usize] == 0 {
                cache[*p as usize] = tree.blend(*p, sf.tint, sf.tint_pct as i32);
            }
            *p = cache[*p as usize];
        }
    }
    super::draw_unit(game, cast.caster, 0);
    if let Some(p) = combo_partner(cast, sp) {
        super::draw_unit(game, p, 0);
    }
    for &(x, y) in tg.cells() {
        match game.battle.unit_at_cell(cast.side.other(), x, y, false) {
            Some(u) => {
                if sp.target != 1 {
                    super::draw_unit(game, u, 0);
                }
            }
            None => {
                if sp.target != 2 {
                    if let Some(u) = game.battle.unit_at_cell(cast.side, x, y, false) {
                        super::draw_unit(game, u, 0);
                    }
                }
            }
        }
    }
}

/// Port of FUN_004676b9: the effect ripples over the target cells that
/// hold a unit the spell affects (not the casters for a spell on
/// everyone), 3 frames apart.
pub fn ripple_units(game: &mut Game, cast: &Cast, tg: &SpellTarget, effect: usize) {
    let Some(sp) = spell_def(game, cast.spell) else { return };
    let sp = &sp;
    let (x, y) = cast.cell;
    camera::scroll_exact(game, x, y, Some(cast.caster));
    let partner = combo_partner(cast, sp);
    let mut on = Vec::new();
    for &(x, y) in tg.cells() {
        let hit = match game.battle.unit_at_cell(cast.side, x, y, false) {
            None => sp.target != 1 && game.battle.unit_at_cell(cast.side.other(), x, y, false).is_some(),
            Some(u) => sp.target != 2 && (sp.target != 3 || (u != cast.caster && Some(u) != partner)),
        };
        if hit {
            on.push((x, y));
        }
    }
    if on.is_empty() {
        return;
    }
    effect_scene(game, cast, tg, 9);
    let _ = super::play_effect_cascade(game, effect, &on, 2);
}

/// Port of FUN_004679ae: the effect ripples along every target cell (a
/// ray, shape 5, without its first cell), 2 frames apart.
pub fn ripple_cells(game: &mut Game, cast: &Cast, tg: &SpellTarget, effect: usize) {
    let Some(sp) = spell_def(game, cast.spell) else { return };
    let sp = &sp;
    let (x, y) = cast.cell;
    camera::scroll_exact(game, x, y, Some(cast.caster));
    let cells = tg.cells();
    let skip = usize::from(sp.shape == 5);
    let on = cells.get(skip..).unwrap_or(&[]).to_vec();
    if on.is_empty() {
        return;
    }
    effect_scene(game, cast, tg, 10);
    let _ = super::play_effect_cascade(game, effect, &on, 1);
}
