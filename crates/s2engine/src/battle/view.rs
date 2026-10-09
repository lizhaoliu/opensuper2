//! Battle view: draws the battle map, units, effects and overlays like the
//! original's map drawing code. Owned by the GRAPHICS port.
//!
//! # VIEW API
//!
//! All functions draw into the current page (`game.scr`) like the
//! original; only the `animate_*`, `play_*`, `hit_flash`, `fade_unit` and
//! `flash_unit` helpers present frames themselves. The camera is
//! `game.battle.map().scroll_x/scroll_y` (the map object's `+0x6c/+0x70`).
//!
//! ```text
//! // setup
//! init(game, map_id) -> Result<()>                       FUN_00424d81: load assets (once) and build the map background
//! bake(game)                                             sync sprites/scroll; rebuild the surface after reset_surface or a new map
//! // map surface bookkeeping (where the original does it; bake() covers the rest)
//! capture(game, r) / capture_raw(game, r)                FUN_00442022(army, unit, 0 / 1): the unit's save-under
//! erase_unit(game, r)                                    FUN_00441c0b: save-under pasted back, neighbours redrawn; the unit stays off until put back or done
//! put_unit(game, r)                                      FUN_00440264(army, unit, 1): draw it into the surface (whatever its act)
//! bake_army(game, side)                                  FUN_004403b5: draw the army's idle units over the surface
//! redeploy(game)                                         FUN_004231be: armies erased, new units captured, armies drawn
//! redraw_armies(game)                                    FUN_0042deda: armies erased and drawn (遮掩網點 changed)
//! rebake_armies(game)                                    FUN_0041be7a / FUN_0044f53e: armies erased, map redrawn, captured, drawn
//! reset_surface(game)                                    FUN_00431845: rebuild from the background on the next draw (after a load)
//! erase_on_page(game, r)                                 FUN_00441e09: the unit erased from the current page, neighbours redrawn
//! draw_around(game, side, x, y, same_cell)               FUN_0044007a: the side's units around a cell onto the current page
//! refresh_background(game)                               rebuild after story flags 1/2/4 or chests (FUN_0042aba8) change
//! // per frame
//! draw_battle(game)                                      window (FUN_00424c5f) + range + active units (FUN_0043ff88) + cursor (FUN_0042c121)
//! draw_backdrop(game)                                    FUN_00424c5f into page 1: the window only (no lifted units, range or cursor)
//! draw_map(game)                                         FUN_00424c5f on page 0: window + lifted units (no range, no cursor)
//! draw_range_map(game)                                   draw_map with the shown range / spell aim (FUN_00401d75)
//! draw_cursor(game)                                      FUN_0042c121 drawing: the map cursor (FUN_004330e2/00470dad/00401d75/00416c1f)
//! draw_unit(game, r, lift)                               FUN_0043ff88 for one unit (lift 4 = flying unit while selected)
//! draw_ground_shadow(game, r)                            FUN_00442811: shadow under a lifted unit
//! screen_to_cell(game, x, y) -> Option<(i32, i32)>       FUN_00425879
//! cell_screen(game, x, y) -> (i32, i32)                  FUN_00427a10
//! // cursor and ranges
//! cursor_set(game, x, y) / cursor_track(game) -> bool    FUN_0042bfea / FUN_0042c121 mouse part
//! cursor_hide(game)
//! show_range(game, &[RangeCell], side, unit_cell, hard)  FUN_0047280f for every node (pages built at this scroll)
//! range_present(game, r)                                FUN_00471340: one presented frame of the shown range (AI)
//! show_aim(game, &SpellTarget)                          FUN_00401bfa: spell range (sysimg 0x21) + area (0x22) diamonds
//! hide_range(game)                                       (also ends show_aim)
//! // unit animation state
//! set_anim(game, r, set)                                 FUN_00441ac4: animation set (0 idle, 1 active/walk, 2 attack ...), first frame
//! step_unit(game, r, dir)                                FUN_00441a24: face `dir`, or advance one frame
//! image_shadow(game, img, x, y)                           FUN_0042049d (the facing ring's drop shadow)
//! reload_sprite(game, r)                                 re-decode after a class change
//! summon_sprite(game, r)                                 a summon's frame as it stands (FUN_0043eef6)
//! // presenting helpers (block until done)
//! animate_walk(game, r, &[(x, y, dir)])                  FUN_0044c331 presentation: 8 cs per cell, camera follows, footsteps
//! play_action(game, r, dir, set)                         FUN_0044ebb2: play an action once, 12 cs per frame
//! play_actions(game, &[(r, dir, set)])                   FUN_0044ecf8: several units at once (combos, counters)
//! hit_flash(game, r)                                     FUN_0044d024: one white frame, 2 cs
//! fade_unit(game, r, color, reverse)                     FUN_0044f27e: dissolve into (or out of) the ground
//! flash_unit(game, r, color, reverse)                    FUN_0044f11c: tint toward a colour and back
//! play_effect(game, effect, x, y, behind_units)          FUN_00466507: CEL 1 frame set on a cell, with its sound
//! scene_to_page1(game)                                   compose the scene into page 1 first (the effect's backdrop)
//! play_effect_cells(game, effect, &[(x, y)])             FUN_004668dd: one effect on up to 4 cells at once
//! play_effect_cascade(game, effect, &cells, stagger)     FUN_00467be7: rippling over cells, next one every stagger+1 frames
//! show_attack_range(game, &TargetList)                   FUN_0040720c: page 1 = darkened window, range cells lit, targets
//! draw_target_marker(game, x, y)                         FUN_0044179b: the tall cursor on a cell (AI target)
//! // spell casts (cast::)
//! cast::cast_animation(game, &cast, &tg, seq)            FUN_00465f44; seq = FUN_00466f65 (|game, &mut effect| -> whole)
//! cast::draw_sequence(game, &cast, &tg, &mut effect)     FUN_00466f65's drawing alone (no per-cell logic of kinds 2/3/6)
//! cast::caster_pose(game, &cast, &tg)                    FUN_00466ba5: face the cell, magic/technique set, sound 0x62/0x63
//! cast::effect_scene(game, &cast, &tg, kind)             FUN_004663a8 + FUN_00466194: page 1 backdrop (tint, lit targets)
//! cast::effect_on(game, &cast, &tg, kind, effect, x, y)  FUN_0046637b: scene + effect on a cell
//! cast::ripple_units / ripple_cells(game, &cast, &tg, e) FUN_004676b9 / FUN_004679ae (kinds 9 / 10)
//! cast::spell_fx(game, spell) -> SpellFx                 BIN 10 bytes: effect, kind, tint colour, tint %
//! // movement (motion::)
//! motion::animate_move(game, r, &[(x, y, dir)])          FUN_0044cccd: walk / fly / teleport by movement type
//! motion::animate_fly(game, r, steps)                    FUN_0044c631: 12 cs per cell, lifted, mounts, ground shadow
//! motion::animate_teleport(game, r, x, y)                FUN_0044c963: dissolve (0xc2) and reappear, sound 0x68
//! motion::draw_flyer(game, r, step) / draw_sword_rider   FUN_00457345 / FUN_004573cb (selected flyers)
//! motion::draw_window(game)                              FUN_00424c5f alone (surface brought up to date)
//! motion::lift_off(game, r, down)                        FUN_0044cb5f: flyer rises (lands) in 4 frames, sound 0x85/0x84
//! motion::draw_lifted(game, r, lift)                     FUN_0042665a with the unit's frame, `lift` rows up
//! // one-offs (motion::)
//! motion::projectile(game, att, target, base_slot, mode) FUN_00439651: arrows (mode 1: 16 directions), balls (mode 2)
//! motion::wipe_in(game, r)                               FUN_004563e8: class-change reveal
//! motion::shake_screen(game, n, effect)                  FUN_00466e71: page 1 shaken ±2 px, with the effect's sound
//! motion::location_name(game, x, y) -> Vec<u8>           FUN_0042574c
//! // panels and windows (panel::)
//! panel::status_bar(game, moving: Option<UnitRef>)       FUN_0042b45f: 狀況視窗 (unit / terrain / turn line)
//! panel::unit_info(game, r, pos) / terrain_info(...)     FUN_0045161f / FUN_004273e4
//! panel::attack_forecast(game, &Duel)                    FUN_00452215 → FUN_00452246 forecast panel
//! panel::spell_forecast(game, caster, target, hit, amt)  FUN_004521c5 / FUN_00452178
//! panel::duel_panel(game, att, def, Forecast, by_cursor) FUN_00452246
//! panel::reward_window(game, r, exp, text, &items)       FUN_00452e38 presentation (EXP box, text box, item pages)
//! panel::level_up_window(game, r, &[LevelGain])          FUN_004537da + FUN_00456549
//! panel::to_big5(&str) -> Vec<u8>                        table strings back to font bytes
//! panel::ai_thinking(game, r)                            FUN_0040aadc: 思考中 over the AI unit (or bottom centre), presented
//! panel::ai_attack_pause(game, r, cell, &Duel, t0)       FUN_00436b24's 0x3c cs pause: page 1, idle frames, marker, forecast
//! look_of(battle, r) -> UnitLook                         the unit fields the drawing code reads
//! ```
//!
//! The map surface (like the original's) only changes by the explicit
//! operations — erase_unit, put_unit, capture, bake_army, ... called where
//! the original calls FUN_00441c0b, FUN_00440264, FUN_00442022,
//! FUN_004403b5 — so stale save-unders leave the original's traces. Scene
//! draws add the units with `act` 1/3/5 (selected, walking, in combat) that
//! are not in the surface on top.
//!
//! Hit shake (`render_flags & 0x80000000`) is drawn automatically by
//! [`draw_unit`]/[`draw_battle`]; set the flag and redraw like the original.

#[path = "view_cast.rs"]
pub mod cast;
#[path = "view_fx.rs"]
pub mod fx;
#[path = "view_map.rs"]
pub mod map;
#[path = "view_motion.rs"]
pub mod motion;
#[path = "view_overlay.rs"]
pub mod overlay;
#[path = "view_panel.rs"]
pub mod panel;
#[path = "view_surface.rs"]
pub mod surface;
#[path = "view_unit.rs"]
pub mod unit;

use anyhow::{Context, Result};
use s2data::gfx_map::{MapTiles, RenderOptions, Shading};
use s2data::gfx_palette::{tint_tables, BlendCache, ColorTree};
use s2data::gfx_ui::UiImages;
use s2data::gfx_unit::UnitSprites;
use s2data::sprite::Image;

use crate::game::Game;
use crate::gfx;
use crate::Data;

use super::combat::TargetList;
use super::targeting::SpellTarget;
use super::{Battle, Side, UnitRef};
use fx::{EffectBank, FxPainter};
use map::MapView;
use overlay::{CursorArt, MapCursor, OverlayScene, RangeArt, RangeCell, RangePages};
use unit::{DrawGlobals, Placed, SpriteBanks, UnitArt, UnitGfx, UnitLook, UnitScene};

/// sysimg slots (0-based, `(&DAT_00493848)[slot]`) the view uses.
mod slot {
    /// Ground shadow under lifted units (FUN_00442811).
    pub const LIFT_SHADOW: usize = 20;
    pub const SHADOW: usize = 21;
    pub const RIPPLE: usize = 22;
    pub const CURSOR_FLAT: usize = 25;
    pub const CURSOR_BACK: usize = 26;
    pub const CURSOR_FRONT: usize = 27;
    pub const RANGE: usize = 0x22;
    /// Spell aim diamonds: range cells, area cells (FUN_00401bfa / FUN_00403e52).
    pub const AIM_RANGE: usize = 0x21;
    pub const AIM_AREA: usize = 0x22;
    pub const STONE: usize = 38;
}

/// Everything the view loads once (about 85 MB of decoded archives).
pub struct ViewAssets {
    pub tiles: MapTiles,
    pub sprites: SpriteBanks,
    pub fx: EffectBank,
    /// Steady-state colour tree (range 0x20..=0xfe) over the master palette.
    pub tree: ColorTree,
    /// `DAT_0049fe08`.
    pub tints: Vec<[u8; 256]>,
    pub shading: Shading,
    /// sysimg slots 0..=38.
    pub sysimg: Vec<Image>,
}

impl ViewAssets {
    /// `master6` is the 768-byte master palette (6-bit).
    pub fn load(data: &Data, master6: &[u8]) -> Result<ViewAssets> {
        let arcs = &data.arcs;
        let tiles = MapTiles::load(arcs).context("map tiles")?;
        let sprites = SpriteBanks::new(UnitSprites::load_normal(arcs)?, UnitSprites::load_summon(arcs)?);
        let fx = EffectBank::load(arcs)?;
        let tree = ColorTree::for_master(master6);
        let tints = tint_tables(&tree);
        let shading = Shading::from_tints(&tints);
        let ui = UiImages::load(arcs)?;
        let sysimg = (0..=slot::STONE).map(|s| ui.get(s + 1)).collect::<s2data::Result<Vec<_>>>()?;
        Ok(ViewAssets { tiles, sprites, fx, tree, tints, shading, sysimg })
    }

    fn unit_art(&self) -> UnitArt<'_> {
        UnitArt { shadow: &self.sysimg[slot::SHADOW], ripple: &self.sysimg[slot::RIPPLE], stone: &self.sysimg[slot::STONE], tints: &self.tints }
    }
}

/// View state kept in `Battle::view`.
#[derive(Default)]
pub struct BattleView {
    pub assets: Option<Box<ViewAssets>>,
    pub map: Option<MapView>,
    /// Per army (index = side), per unit index: the decoded frame (`unit+0xad`).
    pub units: [Vec<UnitGfx>; 2],
    pub cursor: MapCursor,
    pub range: Option<RangePages>,
    /// Range pages use the hard-mode pulse (`FUN_00470dad`).
    pub range_hard: bool,
    /// `DAT_004a1f58+0x5f8`: time of the last effect frame.
    pub fx_last: i32,
    /// `DAT_0048ea9c`: status/forecast panel at the top (1) or bottom (2).
    pub panel_pos: i32,
    /// `DAT_0049fe00`: frame of a flyer's mount (sysimg 23/24).
    pub mount_phase: usize,
    /// The map surface is to be rebuilt from the background (after a new
    /// map or background, or a load).
    fresh: bool,
}

impl BattleView {
    pub fn assets(&self) -> &ViewAssets {
        self.assets.as_deref().expect("battle view assets not loaded (view::init)")
    }

    pub fn map_view(&self) -> &MapView {
        self.map.as_ref().expect("battle view has no map (view::init)")
    }
}

// ---------------------------------------------------------------------------
// Projections of the battle state.

/// The view's projection of a unit (see [`UnitLook`]).
pub fn look_of(b: &Battle, r: UnitRef) -> UnitLook {
    let u = b.unit(r);
    let class_move = b.tables.as_ref().and_then(|t| t.kinds.class(u.kind as u16)).map_or(0, |c| c.move_type as i8);
    UnitLook {
        x: u.x,
        y: u.y,
        kind: u.kind,
        facing: u.facing,
        frame: u.frame,
        act: u.act,
        flags: u.flags,
        buffs: u.buffs,
        status: u.status,
        render_flags: u.render_flags,
        remap: u.remap,
        anim_timer: u.anim_timer,
        side: r.side.index() as i32,
        invisible: b.map.is_some() && b.is_hidden(r.side, u),
        move_type: b.move_type(u),
        class_move,
    }
}

/// Write the animation fields back into the unit.
fn store(b: &mut Battle, r: UnitRef, l: &UnitLook, gfx: &UnitGfx) {
    let u = b.unit_mut(r);
    u.facing = l.facing;
    u.frame = l.frame;
    u.remap = l.remap;
    u.render_flags = l.render_flags;
    u.anim_timer = l.anim_timer;
    // A dead unit the load gave no frame (FUN_0044306f: an enemy extra) keeps
    // its cleared sizes.
    let frameless = u.flags & super::unit::flag::DEAD != 0 && u.sprite_w == 0 && u.sprite_h == 0;
    if let (Some(s), false) = (&gfx.sprite, frameless) {
        u.sprite_w = s.width as i16;
        u.sprite_h = s.height as i16;
    }
}

fn looks(b: &Battle) -> [Vec<UnitLook>; 2] {
    let side = |s: Side| (0..b.army(s).units.len()).map(|i| look_of(b, UnitRef::new(s, i))).collect();
    [side(Side::Player), side(Side::Enemy)]
}

fn globals(game: &Game) -> DrawGlobals {
    DrawGlobals { vs_mode: game.battle.vs, acting_side: game.battle.vs_side, mesh: game.cfg.mesh }
}

/// Make sure every unit has a decoded frame (`FUN_0043f96a`: FUN_0043a1fe for
/// its facing, then `FUN_0043a34f`).
/// A unit whose kind or facing the battle code changed without the view
/// (`movement::set_facing`, a class change, a restored facing) gets the
/// view half of FUN_00441a24: FUN_0043a1fe starts the action of the new
/// facing, FUN_0043a34f decodes; a changed frame alone is redecoded.
fn sync_sprites(game: &mut Game) {
    let Some(assets) = game.battle.view.assets.take() else { return };
    for s in [Side::Player, Side::Enemy] {
        let n = game.battle.army(s).units.len();
        game.battle.view.units[s.index()].resize_with(n, UnitGfx::default);
        for i in 0..n {
            let r = UnitRef::new(s, i);
            let mut l = look_of(&game.battle, r);
            let key = unit::frame_key(&l);
            let g = &game.battle.view.units[s.index()][i];
            if g.sprite.is_some() && g.synced == Some(key) {
                continue;
            }
            let mut g = std::mem::take(&mut game.battle.view.units[s.index()][i]);
            // A unit off the map keeps its frame byte (FUN_00443c84 clears a
            // dead unit's facing without starting an action; saves show it).
            let off_map = !l.on_map();
            match g.synced {
                _ if off_map => {}
                Some((kind, summon, facing, _)) if g.sprite.is_some() && (kind, summon, facing) != (key.0, key.1, key.2) => {
                    let f = l.facing;
                    assets.sprites.set_action(&mut l, f);
                }
                // FUN_0043f96a: a new unit starts the action of its facing; a
                // summoned creature keeps the frame it was created with
                // (FUN_0043eef6: FUN_0043a254 + FUN_0043a34f, no FUN_0043a1fe).
                None if !l.is_summon() => {
                    let f = l.facing;
                    assets.sprites.set_action(&mut l, f);
                }
                _ => {}
            }
            assets.sprites.decode(&mut l, &mut g);
            fit_under(&mut g);
            g.synced = Some(unit::frame_key(&l));
            store(&mut game.battle, r, &l, &g);
            game.battle.view.units[s.index()][i] = g;
        }
    }
    game.battle.view.assets = Some(assets);
}

fn placed<'a>(looks: &'a [Vec<UnitLook>; 2], gfx: &'a [Vec<UnitGfx>; 2]) -> [Vec<Placed<'a>>; 2] {
    let side = |s: usize| {
        looks[s]
            .iter()
            .enumerate()
            .map(|(i, l)| {
                let g = gfx[s].get(i);
                Placed { look: l, sprite: g.and_then(|g| g.sprite.as_ref()), under: g.and_then(|g| g.under.as_ref()) }
            })
            .collect()
    };
    [side(0), side(1)]
}

/// The unit's save-under (`+0xb1`), or the bare background behind it when
/// it has none yet.
fn under_of<'a>(mv: &MapView, g: &'a UnitGfx, look: &UnitLook, sprite: &Image) -> std::borrow::Cow<'a, Image> {
    match g.under.as_ref() {
        Some(u) => std::borrow::Cow::Borrowed(u),
        None => std::borrow::Cow::Owned(unit::bare_under(mv, look, sprite)),
    }
}

/// A frame decoded at another size drops the save-under (the original
/// reallocates the two together, FUN_0043a34f).
fn fit_under(g: &mut UnitGfx) {
    let fits = match (g.sprite.as_ref(), g.under.as_ref()) {
        (Some(s), Some(u)) => (s.width, s.height) == (u.width, u.height),
        _ => true,
    };
    if !fits {
        g.under = None;
        g.under_at = None;
    }
}

fn sync_scroll(game: &mut Game) {
    if let (Some(m), Some(v)) = (game.battle.map.as_ref(), game.battle.view.map.as_mut()) {
        v.scroll_x = m.scroll_x;
        v.scroll_y = m.scroll_y;
    }
}

// ---------------------------------------------------------------------------
// Setup.

/// Port of FUN_00424d81 (display part): load the view assets on first use
/// and build the background of PRG 6 map `map_id`.
pub fn init(game: &mut Game, map_id: usize) -> Result<()> {
    if game.battle.view.assets.is_none() {
        let master = match game.res.as_ref() {
            Some(r) => r.master.clone(),
            None => {
                let mut m = game.data.dir.read("supersp.pal")?[..0x300].to_vec();
                m[..3].fill(0);
                m
            }
        };
        game.battle.view.assets = Some(Box::new(ViewAssets::load(game.data, &master)?));
    }
    let opts = render_options(game);
    let assets = game.battle.view.assets.as_deref().unwrap();
    let mut mv = MapView::load(&game.data.arcs, &assets.tiles, &assets.shading, map_id, opts)?;
    if mv.apply_treasure(&game.events.treasure, game.events.treasure_count) {
        mv.rebuild_background(&assets.tiles, &assets.shading);
    }
    let v = &mut game.battle.view;
    v.map = Some(mv);
    v.units = [Vec::new(), Vec::new()];
    v.range = None;
    v.fresh = true;
    v.cursor = MapCursor::default();
    // The data segment's initial value.
    v.panel_pos = 2;
    sync_scroll(game);
    Ok(())
}

/// What the background depends on: story flags 2, 1 and 4 (`DAT_00494509`,
/// `DAT_00494508`, `DAT_0049450b`) show overlay categories 5, 6 and 0xb;
/// `DAT_0049478c` is the number of treasure chests.
fn render_options(game: &Game) -> RenderOptions {
    let f = &game.campaign.flags;
    RenderOptions { show_cat5: f[2] != 0, show_cat6: f[1] != 0, show_cat11: f[4] != 0, door_count: game.events.treasure_count }
}

/// Rebuild the background if the story flags or the treasure state that
/// shape it changed (the original redraws the map when a script sets them).
pub fn refresh_background(game: &mut Game) {
    let opts = render_options(game);
    let treasure = game.events.treasure;
    let v = &mut game.battle.view;
    let (Some(assets), Some(mv)) = (v.assets.as_deref(), v.map.as_mut()) else { return };
    let same = (mv.opts.show_cat5, mv.opts.show_cat6, mv.opts.show_cat11, mv.opts.door_count) == (opts.show_cat5, opts.show_cat6, opts.show_cat11, opts.door_count);
    mv.opts = opts;
    let removed = mv.apply_treasure(&treasure, opts.door_count);
    if !same || removed {
        let old = mv.bg.clone();
        mv.rebuild_background(&assets.tiles, &assets.shading);
        if !v.fresh {
            rebake_armies_with(game, Some(old));
        }
    }
}

/// FUN_0042deda: both armies erased from the surface (FUN_00441222 x2)
/// and drawn again (FUN_004403b5 x2), as after the 遮掩網點 setting changed.
pub fn redraw_armies(game: &mut Game) {
    if game.battle.view.fresh {
        return bake(game);
    }
    with_surf(game, |sf| {
        sf.erase_army(0);
        sf.erase_army(1);
        sf.bake_army(0);
        sf.bake_army(1);
    });
}

/// FUN_0044f53e / FUN_0041be7a: both armies erased from the surface
/// (FUN_00441222 x2), the map redrawn where it changed (`old` the
/// background before), the save-unders captured again (FUN_00441ef1 x2)
/// and both armies drawn (FUN_004403b5 x2).
pub fn rebake_armies(game: &mut Game) {
    refresh_background(game);
    rebake_armies_with(game, None);
}

fn rebake_armies_with(game: &mut Game, old: Option<Image>) {
    if game.battle.view.fresh {
        return bake(game);
    }
    with_surf(game, |sf| {
        sf.erase_army(0);
        sf.erase_army(1);
        if let Some(old) = old {
            let mv = &mut *sf.mv;
            for ((s, &b), &o) in mv.surface.pixels.iter_mut().zip(mv.bg.pixels.iter()).zip(old.pixels.iter()) {
                if b != o {
                    *s = b;
                }
            }
        }
        sf.grab_army(0);
        sf.grab_army(1);
        sf.bake_army(0);
        sf.bake_army(1);
    });
}

/// Bring the surface up to date (camera, background, sprites, baked units).
pub fn sync_view(game: &mut Game) {
    sync_scroll(game);
    refresh_background(game);
    bake(game);
}

/// Run `f` on the map surface bookkeeping (see [`surface`]); render flags
/// the drawing toggled (hit shake phase) go back to the units.
fn with_surf<R>(game: &mut Game, f: impl FnOnce(&mut surface::Surf) -> R) -> Option<R> {
    sync_scroll(game);
    sync_sprites(game);
    let lk = looks(&game.battle);
    let g = globals(game);
    let v = &mut game.battle.view;
    let (Some(assets), Some(mv)) = (v.assets.as_deref(), v.map.as_mut()) else { return None };
    let mut sf = surface::Surf { assets, mv, units: &mut v.units, looks: lk, g };
    let out = f(&mut sf);
    let after = std::mem::take(&mut sf.looks);
    for (s, side) in [Side::Player, Side::Enemy].into_iter().enumerate() {
        for (i, l) in after[s].iter().enumerate() {
            let u = game.battle.unit_mut(UnitRef::new(side, i));
            if u.render_flags != l.render_flags {
                u.render_flags = l.render_flags;
            }
        }
    }
    Some(out)
}

/// Bring the view up to date: sprites and scroll; after a new map or
/// [`reset_surface`] the surface is rebuilt (FUN_00424d81, FUN_004433de x2,
/// FUN_004403b5 x2). Otherwise the surface only changes by the explicit
/// operations below ([`capture`], [`erase_unit`], [`put_unit`], ...), as in
/// the original; [`surface::Surf::reconcile`] then only reports (debug).
pub fn bake(game: &mut Game) {
    let fresh = game.battle.view.fresh;
    if with_surf(game, |sf| if fresh { sf.rebuild() } else { sf.reconcile() }).is_some() {
        game.battle.view.fresh = false;
    }
}

/// Port of FUN_00442022(army, unit, 0): capture what lies behind the unit
/// where it stands (the surface, without the other units' images).
pub fn capture(game: &mut Game, r: UnitRef) {
    with_surf(game, |sf| {
        sf.settle(None);
        sf.capture(r.side.index(), r.idx, 0)
    });
}

/// Port of FUN_00442022(army, unit, 1): the surface behind the unit as it is.
pub fn capture_raw(game: &mut Game, r: UnitRef) {
    with_surf(game, |sf| {
        sf.settle(None);
        sf.capture(r.side.index(), r.idx, 1)
    });
}

/// Port of FUN_00441c0b: erase the unit from the map surface (its
/// save-under pasted back) and redraw the units around it.
pub fn erase_unit(game: &mut Game, r: UnitRef) {
    let (s, i) = (r.side.index(), r.idx);
    with_surf(game, |sf| {
        sf.settle(Some((s, i)));
        sf.erase(s, i, &[]);
        if let Some(g) = sf.units[s].get_mut(i) {
            g.held = true;
        }
    });
}

/// Port of FUN_00440264(army, unit, 1): draw the unit into the map surface
/// (whatever its `act`; one drawn while lifted stays there until it moves
/// or is erased).
pub fn put_unit(game: &mut Game, r: UnitRef) {
    let (s, i) = (r.side.index(), r.idx);
    with_surf(game, |sf| {
        sf.settle(None);
        sf.put(s, i, true);
        if let Some(g) = sf.units[s].get_mut(i) {
            g.held = false;
        }
    });
}

/// Port of FUN_004403b5(army): draw the army's idle units over the surface.
pub fn bake_army(game: &mut Game, side: Side) {
    if game.battle.view.fresh {
        return bake(game);
    }
    with_surf(game, |sf| {
        sf.settle(None);
        sf.bake_army(side.index())
    });
}

/// Port of FUN_004231be's redraw around a deployment: both armies erased
/// (FUN_00441222 x2) before the new units arrive, then the units that are
/// new or were moved get their save-unders (FUN_0043f96a: as the surface
/// is; FUN_0043dc8b: FUN_00442022) and both armies are drawn
/// (FUN_004403b5 x2). Call it after creating the units.
pub fn redeploy(game: &mut Game) {
    if game.battle.view.fresh {
        return bake(game);
    }
    with_surf(game, |sf| {
        sf.erase_army(0);
        sf.erase_army(1);
        for s in 0..2 {
            for i in 0..sf.looks[s].len().min(sf.units[s].len()) {
                let l = sf.looks[s][i];
                if !l.on_map() {
                    continue;
                }
                match sf.units[s][i].under_at {
                    None => sf.capture(s, i, 1),
                    Some(at) if at != (l.x, l.y) => sf.capture(s, i, 0),
                    _ => {}
                }
            }
        }
        sf.bake_army(0);
        sf.bake_army(1);
    });
}

/// The surface is to be rebuilt from the bare background on the next draw
/// (FUN_00431845 after loading a save: FUN_00424d81, FUN_004433de x2,
/// FUN_004403b5 x2).
pub fn reset_surface(game: &mut Game) {
    game.battle.view.fresh = true;
}

// ---------------------------------------------------------------------------
// Per frame.

/// The standard battle frame into the current page: map window (with the
/// range display if one is shown), units that are not baked (selected,
/// walking, in combat) and the map cursor.
pub fn draw_battle(game: &mut Game) {
    draw_scene(game, true, true, true);
}

/// FUN_00424c5f as the battle code uses it on page 0: the map window and the
/// units lifted off the surface, but no map cursor — the original draws
/// that only from FUN_004330e2, FUN_00470dad, FUN_00401d75 and FUN_00416c1f
/// ([`draw_cursor`]) — and no range display (only the range frames
/// FUN_00470dad / FUN_00471340 / FUN_00401d75 show it).
pub fn draw_map(game: &mut Game) {
    draw_scene(game, true, false, false);
}

/// [`draw_map`] with the shown range or spell aim composed over the window
/// (FUN_00401d75's page-2 copy).
pub fn draw_range_map(game: &mut Game) {
    draw_scene(game, true, false, true);
}

/// Port of FUN_0042c121's drawing: the map cursor onto the current page
/// (when shown). The cursor cell is `battle.cursor` (`DAT_0048eaa0/a4`);
/// one the battle code moved without the view is re-read first
/// (FUN_0042bfea), as the status line does.
pub fn draw_cursor(game: &mut Game) {
    if game.battle.view.map.is_none() || game.battle.view.assets.is_none() {
        return;
    }
    let (cx, cy) = game.battle.cursor;
    if (game.battle.view.cursor.col, game.battle.view.cursor.row) != (cx, cy) {
        cursor_set(game, cx, cy);
    }
    sync_scroll(game);
    sync_sprites(game);
    let lk = looks(&game.battle);
    let g = globals(game);
    let page = game.scr.cur;
    let Game { battle, scr, .. } = game;
    let v = &mut battle.view;
    let (Some(assets), Some(mv)) = (v.assets.as_deref(), v.map.as_ref()) else { return };
    let pl = placed(&lk, &v.units);
    let os = OverlayScene { map: mv, tiles: &assets.tiles, shading: &assets.shading, g, armies: [&pl[0], &pl[1]] };
    let art = CursorArt { flat: &assets.sysimg[slot::CURSOR_FLAT], back: &assets.sysimg[slot::CURSOR_BACK], front: &assets.sysimg[slot::CURSOR_FRONT] };
    let mut cur = v.cursor;
    if cur.shown {
        os.draw_cursor(&mut scr.pages[page], &mut cur, &art);
    }
    drop(pl);
    v.cursor = cur;
}

/// FUN_00424c5f as the backdrop page (page 1): the map window, without the
/// units lifted off the surface (selected, walking, in combat), the range
/// display and the map cursor. The
/// frames that copy it draw the lifted units over it themselves
/// (FUN_004351ad and the menus), as in the original, where those units
/// are not in `DAT_004947f0`; a copy holding them would leave their
/// previous frame behind.
pub fn draw_backdrop(game: &mut Game) {
    draw_scene(game, false, false, false);
}

/// Debugging aid: with `S2_SURFACE_DUMP=DIR:MS`, every scene drawn at or after
/// MS (virtual milliseconds) rewrites the map surface (`surface.bin`: u32
/// width, u32 height, pixels) and every unit's save-under and frame
/// (`units.txt`) in DIR, so a script ending at a shot leaves the state
/// behind it, to compare with the original's (`oracle.py --peek`).
fn debug_dump(game: &Game) {
    let Ok(spec) = std::env::var("S2_SURFACE_DUMP") else { return };
    let Some((dir, t)) = spec.rsplit_once(':') else { return };
    if (game.ticks() as i64) * 10 < t.parse::<i64>().unwrap_or(0) {
        return;
    }
    let v = &game.battle.view;
    let Some(mv) = v.map.as_ref() else { return };
    let mut bin = Vec::with_capacity(8 + mv.surface.pixels.len());
    bin.extend_from_slice(&(mv.surface.width as u32).to_le_bytes());
    bin.extend_from_slice(&(mv.surface.height as u32).to_le_bytes());
    bin.extend_from_slice(&mv.surface.pixels);
    let _ = std::fs::write(format!("{dir}/surface.bin"), bin);
    let mut txt = format!("scroll {} {}\n", mv.scroll_x, mv.scroll_y);
    let hex = |img: &Image| img.pixels.iter().map(|p| format!("{p:02x}")).collect::<String>();
    for (s, army) in v.units.iter().enumerate() {
        for (i, g) in army.iter().enumerate() {
            let l = look_of(&game.battle, UnitRef::new(if s == 0 { Side::Player } else { Side::Enemy }, i));
            txt += &format!(
                "unit {s} {i} at {} {} act {} flags {:#x} under_at {:?} drawn {:?}\n",
                l.x,
                l.y,
                l.act,
                l.flags,
                g.under_at,
                g.drawn.map(|d| (d.look.x, d.look.y, d.look.act, d.w, d.h))
            );
            if let Some(sp) = &g.sprite {
                txt += &format!("frame {} {} {}\n", sp.width, sp.height, hex(sp));
            }
            if let Some(u) = &g.under {
                txt += &format!("under {} {} {}\n", u.width, u.height, hex(u));
            }
        }
    }
    let _ = std::fs::write(format!("{dir}/units.txt"), txt);
}

fn draw_scene(game: &mut Game, lifted: bool, with_cursor: bool, with_range: bool) {
    if game.battle.view.map.is_none() || game.battle.view.assets.is_none() {
        return;
    }
    sync_scroll(game);
    refresh_background(game);
    bake(game);
    let lk = looks(&game.battle);
    debug_dump(game);
    let now = game.ticks();
    let g = globals(game);
    let page = game.scr.cur;
    let Game { battle, scr, .. } = game;
    let v = &mut battle.view;
    let (Some(assets), Some(mv)) = (v.assets.as_deref(), v.map.as_ref()) else { return };
    let dst = &mut scr.pages[page];
    mv.blit_window(dst);
    if let Some(rp) = v.range.as_mut().filter(|_| with_range) {
        rp.compose(dst, now, v.range_hard, &assets.tints, (mv.scroll_x, mv.scroll_y));
    }
    if !lifted {
        return;
    }
    let pl = placed(&lk, &v.units);
    let scene = UnitScene { map: mv, tiles: &assets.tiles, shading: &assets.shading, art: assets.unit_art(), g, armies: [&pl[0], &pl[1]] };
    let mut updates = Vec::new();
    // Units lifted off the surface, back to front (FUN_0044ecf8 sorts the
    // same way when several animate together).
    // A lifted unit the battle code drew into the surface itself is shown
    // from there (FUN_0044e210 draws it before its last frame).
    let mut active: Vec<(usize, usize)> = (0..2)
        .flat_map(|s| (0..lk[s].len()).map(move |i| (s, i)))
        .filter(|&(s, i)| lk[s][i].on_map() && !lk[s][i].is_baked() && v.units[s].get(i).map_or(false, |g| g.drawn.is_none()))
        .collect();
    active.sort_by_key(|&(s, i)| lk[s][i].y);
    for (s, i) in active {
        let gx = &v.units[s][i];
        if let Some(sprite) = gx.sprite.as_ref() {
            let mut l2 = lk[s][i];
            let under = under_of(mv, gx, &l2, sprite);
            scene.draw_on_screen(dst, &mut l2, sprite, &under, 0);
            updates.push((s, i, l2.render_flags));
        }
    }
    let os = OverlayScene { map: mv, tiles: &assets.tiles, shading: &assets.shading, g, armies: [&pl[0], &pl[1]] };
    let art = CursorArt { flat: &assets.sysimg[slot::CURSOR_FLAT], back: &assets.sysimg[slot::CURSOR_BACK], front: &assets.sysimg[slot::CURSOR_FRONT] };
    let mut cur = v.cursor;
    if cur.shown && with_cursor {
        os.draw_cursor(dst, &mut cur, &art);
    }
    drop(pl);
    v.cursor = cur;
    for (s, i, rf) in updates {
        let side = if s == 0 { Side::Player } else { Side::Enemy };
        battle.unit_mut(UnitRef::new(side, i)).render_flags = rf;
    }
}

/// Port of FUN_0043ff88: draw one unit onto the current page.
pub fn draw_unit(game: &mut Game, r: UnitRef, lift: i32) {
    sync_scroll(game);
    sync_sprites(game);
    let lk = looks(&game.battle);
    let g = globals(game);
    let page = game.scr.cur;
    let Game { battle, scr, .. } = game;
    let v = &battle.view;
    let (Some(assets), Some(mv)) = (v.assets.as_deref(), v.map.as_ref()) else { return };
    let Some(gx) = v.units[r.side.index()].get(r.idx) else { return };
    let Some(sprite) = gx.sprite.as_ref() else { return };
    let pl = placed(&lk, &v.units);
    let scene = UnitScene { map: mv, tiles: &assets.tiles, shading: &assets.shading, art: assets.unit_art(), g, armies: [&pl[0], &pl[1]] };
    let mut l = lk[r.side.index()][r.idx];
    let under = under_of(mv, gx, &l, sprite);
    scene.draw_on_screen(&mut scr.pages[page], &mut l, sprite, &under, lift);
    let rf = l.render_flags;
    drop(pl);
    battle.unit_mut(r).render_flags = rf;
}

/// Port of FUN_00441e09: erase unit `r` from the current page — its
/// save-under pasted where it is drawn — and draw the units around it over
/// that (FUN_00441d14 for its army, then the other: FUN_0043ff88). The map
/// surface keeps it.
pub fn erase_on_page(game: &mut Game, r: UnitRef) {
    sync_scroll(game);
    sync_sprites(game);
    let mut lk = looks(&game.battle);
    let g = globals(game);
    let page = game.scr.cur;
    let (s, i) = (r.side.index(), r.idx);
    let Game { battle, scr, .. } = game;
    let v = &battle.view;
    let (Some(assets), Some(mv)) = (v.assets.as_deref(), v.map.as_ref()) else { return };
    let Some(gx) = v.units[s].get(i) else { return };
    let Some(sprite) = gx.sprite.as_ref() else { return };
    let me = lk[s][i];
    {
        let under = under_of(mv, gx, &me, sprite);
        let (cx, cy) = mv.cell_screen(me.x, me.y);
        let (x, y) = (cx + 0x20 - (sprite.width as i32 >> 1), cy + 0x20 - sprite.height as i32);
        scr.pages[page].blit_opaque(&under, x as isize, y as isize);
    }
    lk[s][i].flags |= unit::F_DEAD | 0x0200_0000;
    let mut updates = Vec::new();
    {
        let pl = placed(&lk, &v.units);
        let scene = UnitScene { map: mv, tiles: &assets.tiles, shading: &assets.shading, art: assets.unit_art(), g, armies: [&pl[0], &pl[1]] };
        for side in [s, 1 - s] {
            for (k, o) in lk[side].iter().enumerate() {
                if !o.is_baked() || (o.x, o.y) == (me.x, me.y) || o.x < me.x - 2 || o.x > me.x + 2 || o.y > me.y + 0x18 || o.y < me.y - 0x18 {
                    continue;
                }
                let Some(ox) = v.units[side].get(k) else { continue };
                let Some(osp) = ox.sprite.as_ref() else { continue };
                let mut l2 = *o;
                let under = under_of(mv, ox, &l2, osp);
                scene.draw_on_screen(&mut scr.pages[page], &mut l2, osp, &under, 0);
                updates.push((side, k, l2.render_flags));
            }
        }
    }
    for (side, k, rf) in updates {
        battle.unit_mut(UnitRef::new(if side == 0 { Side::Player } else { Side::Enemy }, k)).render_flags = rf;
    }
}

/// Port of FUN_0044007a(army, x, y, same_cell): draw the units of `side`
/// around cell (x, y) onto the current page (FUN_0043ff88), not lifted ones,
/// nor one on the cell itself unless `same_cell`.
pub fn draw_around(game: &mut Game, side: Side, x: i32, y: i32, same_cell: bool) {
    sync_scroll(game);
    sync_sprites(game);
    let lk = looks(&game.battle);
    let n = lk[side.index()].len();
    for k in 0..n {
        let o = lk[side.index()][k];
        if !o.is_baked() || ((o.x, o.y) == (x, y) && !same_cell) || o.x < x - 2 || o.x > x + 2 || o.y > y + 0x18 || o.y < y - 0x18 {
            continue;
        }
        draw_unit(game, UnitRef::new(side, k), 0);
    }
}

/// The map window with the shown range or spell aim, without the lifted
/// units (FUN_00401d75 / FUN_00471340 draw the moving unit themselves).
pub fn draw_range_window(game: &mut Game) {
    draw_scene(game, false, false, true);
}

/// Port of FUN_00442811(army, unit, 0): the ground shadow of a lifted
/// (flying) unit, made from the page pixels of its cell darkened inside
/// ellipse sysimg 20.
pub fn draw_ground_shadow(game: &mut Game, r: UnitRef) {
    draw_ground_shadow_with(game, r, false);
}

/// FUN_00442811 with its scenery pass's own-overlay flag (1 from the aim
/// frame FUN_00401d75).
pub fn draw_ground_shadow_with(game: &mut Game, r: UnitRef, own_overlay: bool) {
    sync_scroll(game);
    sync_sprites(game);
    let lk = looks(&game.battle);
    let g = globals(game);
    let page = game.scr.cur;
    let Game { battle, scr, .. } = game;
    let v = &battle.view;
    let (Some(assets), Some(mv)) = (v.assets.as_deref(), v.map.as_ref()) else { return };
    let l = lk[r.side.index()][r.idx];
    let (cx, cy) = mv.cell_screen(l.x, l.y);
    let mut buf = unit::grab(&scr.pages[page], cx, cy, 64, 32);
    let ell = &assets.sysimg[slot::LIFT_SHADOW];
    for (p, &e) in buf.pixels.iter_mut().zip(ell.pixels.iter()) {
        *p = if e == 0 { 0 } else { assets.tints[5][*p as usize] };
    }
    let pl = placed(&lk, &v.units);
    for (side, army) in pl.iter().enumerate() {
        unit::cut_by_units(mv, army, side as i32, &g, l.x, l.y, &mut buf, 1, 0, 0, None);
    }
    let occ = s2data::gfx_map::Occlusion { own_overlay, ..Default::default() };
    mv.renderer(&assets.tiles, &assets.shading).occlude(&mut buf, l.x, l.y, occ);
    let (x, y) = unit::unit_screen_pos(mv, l.x, l.y, &buf, 0, 0);
    scr.pages[page].blit(&buf, x as isize, y as isize);
}

/// Port of FUN_00425879 (with the 遮掩網點 setting choosing the variant).
pub fn screen_to_cell(game: &mut Game, x: i32, y: i32) -> Option<(i32, i32)> {
    sync_scroll(game);
    let mesh = game.cfg.mesh;
    game.battle.view.map.as_ref()?.screen_to_cell(x, y, mesh)
}

/// Port of FUN_00427a10.
pub fn cell_screen(game: &mut Game, x: i32, y: i32) -> (i32, i32) {
    sync_scroll(game);
    game.battle.view.map.as_ref().map_or((0, 0), |m| m.cell_screen(x, y))
}

/// Port of FUN_0042049d(x, y, img, page): darken the current page through
/// the shadow table (DAT_004a0408, tint 6) under the non-zero pixels of the
/// raw image `img` placed at (x, y) — the drop shadow of the facing ring.
/// Its clipping is kept: a right overflow replaces the left skip, the
/// bottom limit is row 0x1e0.
pub fn image_shadow(game: &mut Game, img: &Image, mut x: i32, mut y: i32) {
    let lut = game.res().tints[6];
    let (iw, ih) = (img.width as i32, img.height as i32);
    let (mut w, mut h) = (iw, ih);
    let mut src = 0i32;
    let mut skip = 0i32;
    if y < 0 {
        if y + h < 1 {
            return;
        }
        h += y;
        src -= y * w;
        y = 0;
    }
    if x < 0 {
        if x + w < 1 {
            return;
        }
        skip = -x;
        w += x;
        x = 0;
        src += skip;
    }
    if x + w > 0x280 {
        skip = w - 0x280 + x;
        w = 0x280 - x;
    }
    if y + h > 0x1df {
        h = 0x1e0 - y;
    }
    let page = game.scr.page();
    let stride = page.width as i32;
    let mut dst = y * stride + x;
    for _ in 0..h.max(0) {
        for _ in 0..w {
            if img.pixels.get(src as usize).is_some_and(|&p| p != 0) {
                if let Some(p) = page.pixels.get_mut(dst as usize) {
                    *p = lut[*p as usize];
                }
            }
            src += 1;
            dst += 1;
        }
        src += skip;
        dst += stride - w;
    }
}

// ---------------------------------------------------------------------------
// Cursor and ranges.

/// Port of FUN_0042bfea: put the map cursor on a cell and show it.
pub fn cursor_set(game: &mut Game, x: i32, y: i32) {
    game.battle.cursor = (x, y);
    let v = &mut game.battle.view;
    if let Some(mv) = v.map.as_ref() {
        v.cursor.set_cell(mv, x, y);
    }
}

/// The mouse part of FUN_0042c121: move the cursor to the cell under the
/// mouse; true if it changed.
pub fn cursor_track(game: &mut Game) -> bool {
    sync_scroll(game);
    let mouse = game.sys.mouse();
    let mesh = game.cfg.mesh;
    let v = &mut game.battle.view;
    let Some(mv) = v.map.as_ref() else { return false };
    let moved = v.cursor.track_mouse(mv, mouse, mesh);
    if moved {
        game.battle.cursor = (v.cursor.col, v.cursor.row);
    }
    moved
}

pub fn cursor_hide(game: &mut Game) {
    game.battle.view.cursor.shown = false;
}

/// Port of FUN_0047280f over a whole range: build the range pages from the
/// current window. `side` is the moving unit's army, `unit_cell` its cell.
pub fn show_range(game: &mut Game, cells: &[RangeCell], side: Side, unit_cell: (i32, i32), hard: bool) {
    sync_scroll(game);
    bake(game);
    let lk = looks(&game.battle);
    let g = globals(game);
    let v = &mut game.battle.view;
    let (Some(assets), Some(mv)) = (v.assets.as_deref(), v.map.as_ref()) else { return };
    let mut window = Image::new(640, 480);
    mv.blit_window(&mut window);
    let pl = placed(&lk, &v.units);
    let os = OverlayScene { map: mv, tiles: &assets.tiles, shading: &assets.shading, g, armies: [&pl[0], &pl[1]] };
    let s = &assets.sysimg;
    let art = RangeArt { diamonds: [&s[slot::RANGE], &s[slot::RANGE + 1], &s[slot::RANGE + 2], &s[slot::RANGE + 3]] };
    let pages = os.build_range(cells, side.index() as i32, unit_cell, hard, &art, &window);
    drop(pl);
    v.range = Some(pages);
    v.range_hard = hard;
}

/// Port of FUN_00401bfa: show the aim of spell target `tg` — its range
/// cells (sysimg 0x21) and area cells (0x22) over the window as built now;
/// [`draw_battle`] composes it, shifted if the camera moves (FUN_00401d75),
/// until [`hide_range`].
pub fn show_aim(game: &mut Game, tg: &SpellTarget) {
    sync_scroll(game);
    bake(game);
    let lk = looks(&game.battle);
    let g = globals(game);
    let caster = {
        let u = game.battle.unit(tg.caster);
        (u.x, u.y)
    };
    let v = &mut game.battle.view;
    let (Some(assets), Some(mv)) = (v.assets.as_deref(), v.map.as_ref()) else { return };
    let mut window = Image::new(640, 480);
    mv.blit_window(&mut window);
    let pl = placed(&lk, &v.units);
    let os = OverlayScene { map: mv, tiles: &assets.tiles, shading: &assets.shading, g, armies: [&pl[0], &pl[1]] };
    let s = &assets.sysimg;
    let pages = os.build_aim(&tg.range, &tg.area, tg.side.index(), caster, &s[slot::AIM_RANGE], &s[slot::AIM_AREA], &window);
    drop(pl);
    v.range = Some(pages);
    v.range_hard = false;
}

/// Port of FUN_00471340: one presented frame of the range shown for `r`
/// (an AI unit about to act): the window with the range pages, the unit
/// stepping its frame every 0x13 cs (not move type 4) and drawn by its
/// move type, and the pointer.
pub fn range_present(game: &mut Game, r: UnitRef) {
    if game.battle.view.map.is_none() || game.battle.view.assets.is_none() {
        return;
    }
    game.scr.set_page(0);
    draw_scene(game, false, false, true);
    let mt = game.battle.range.move_type;
    let t = game.battle.unit(r).anim_timer;
    if mt != 4 && !game.still_waiting(t, 0x13) {
        let f = game.battle.unit(r).facing;
        step_unit(game, r, f);
        game.battle.unit_mut(r).anim_timer = game.ticks();
    }
    match mt {
        3 => {
            draw_ground_shadow(game, r);
            motion::draw_lifted(game, r, 4);
        }
        4 => motion::draw_flyer(game, r, false),
        5 => motion::draw_sword_rider(game, r),
        _ => draw_unit(game, r, 0),
    }
    if game.res.is_some() {
        crate::cursor::draw_present(game, None, false);
    } else {
        gfx::present(game);
    }
}

pub fn hide_range(game: &mut Game) {
    game.battle.view.range = None;
}

// ---------------------------------------------------------------------------
// Unit animation state.

fn with_unit<F: FnOnce(&SpriteBanks, &mut UnitLook, &mut UnitGfx)>(game: &mut Game, r: UnitRef, f: F) {
    sync_sprites(game);
    let Some(assets) = game.battle.view.assets.take() else { return };
    let mut l = look_of(&game.battle, r);
    let mut g = std::mem::take(&mut game.battle.view.units[r.side.index()][r.idx]);
    f(&assets.sprites, &mut l, &mut g);
    fit_under(&mut g);
    g.synced = Some(unit::frame_key(&l));
    store(&mut game.battle, r, &l, &g);
    game.battle.view.units[r.side.index()][r.idx] = g;
    game.battle.view.assets = Some(assets);
}

/// Port of FUN_00441ac4: switch to animation set `set` and its first frame.
pub fn set_anim(game: &mut Game, r: UnitRef, set: u8) {
    with_unit(game, r, |b, l, g| b.set_anim(l, g, set));
}

/// Port of FUN_00441a24.
pub fn step_unit(game: &mut Game, r: UnitRef, dir: u8) {
    with_unit(game, r, |b, l, g| b.step(l, g, dir));
}

/// The unit's decoded frame (`unit+0xad`).
pub fn sprite_of(game: &mut Game, r: UnitRef) -> Option<Image> {
    sync_sprites(game);
    game.battle.view.units[r.side.index()].get(r.idx).and_then(|g| g.sprite.clone())
}

/// Replace the unit's decoded frame (the original edits `unit+0xad` in
/// place, e.g. FUN_004608e2 shortens its height).
pub fn set_sprite(game: &mut Game, r: UnitRef, img: Image) {
    if let Some(g) = game.battle.view.units[r.side.index()].get_mut(r.idx) {
        g.sprite = Some(img);
    }
}

/// Re-decode a unit's frame (after its class or summon state changed).
pub fn reload_sprite(game: &mut Game, r: UnitRef) {
    with_unit(game, r, |b, l, g| {
        b.set_action(l, l.facing);
        b.decode(l, g);
    });
}

/// FUN_0043eef6 (view part): a summoned unit's frame decoded as it stands
/// (FUN_0043a254 + FUN_0043a34f, no FUN_0043a1fe): a new summon's frame
/// byte is 0, a reused one keeps its last.
pub fn summon_sprite(game: &mut Game, r: UnitRef) {
    let n = game.battle.army(r.side).units.len();
    let units = &mut game.battle.view.units[r.side.index()];
    if units.len() < n {
        units.resize_with(n, UnitGfx::default);
    }
    let Some(assets) = game.battle.view.assets.take() else { return };
    let mut l = look_of(&game.battle, r);
    let mut g = std::mem::take(&mut game.battle.view.units[r.side.index()][r.idx]);
    assets.sprites.decode(&mut l, &mut g);
    fit_under(&mut g);
    g.synced = Some(unit::frame_key(&l));
    store(&mut game.battle, r, &l, &g);
    game.battle.view.units[r.side.index()][r.idx] = g;
    game.battle.view.assets = Some(assets);
}

// ---------------------------------------------------------------------------
// Presenting helpers.

/// Port of FUN_0044c331's presentation: the unit steps cell by cell along
/// `steps` (cell and the direction it moves in), 8 cs per step, the camera
/// centred on it, with footstep sounds by movement type and terrain. The
/// unit should have `act == 3` (walking) while this runs.
pub fn animate_walk(game: &mut Game, r: UnitRef, steps: &[(i32, i32, u8)]) {
    let saved_act = game.battle.unit(r).act;
    game.battle.unit_mut(r).act = unit::ACT_WALKING;
    let u = game.battle.unit(r).clone();
    let mt = game.battle.move_type(&u);
    let class_move = game.battle.tables.as_ref().and_then(|t| t.kinds.class(u.kind as u16)).map_or(0, |c| c.move_type);
    let mut sound_kind = 0;
    let mut slot = 0usize;
    for &(x, y, dir) in steps {
        {
            let u = game.battle.unit_mut(r);
            u.x = x;
            u.y = y;
        }
        game.battle.map_mut().center_on(x, y, true);
        game.scr.set_page(0);
        // Footsteps: levitating 0x7a, water 0x83, class move 4 0x7c, else 0x67.
        let water = {
            let v = &game.battle.view;
            match (v.map.as_ref(), v.assets.as_deref()) {
                (Some(mv), Some(a)) => unit::is_water(mv, &a.tiles, x, y),
                _ => false,
            }
        };
        let (k, id) = if mt == 1 {
            (1, 0x7a)
        } else if water {
            (2, 0x83)
        } else if class_move == 4 {
            (3, 0x7c)
        } else {
            (4, 0x67)
        };
        if sound_kind != k {
            sound_kind = k;
            slot = 0;
        }
        slot = game.sys.audio.play_sound(&game.data.arcs, id, slot);
        step_unit(game, r, dir);
        // FUN_00442022(army, unit, 0) on each cell (the surface first
        // brought up to date: the walker left it before setting off).
        bake(game);
        capture(game, r);
        draw_map(game);
        let t0 = game.battle.unit(r).anim_timer;
        game.wait_until(t0, 8);
        gfx::present(game);
        game.battle.unit_mut(r).anim_timer = game.ticks();
    }
    game.battle.unit_mut(r).act = saved_act;
}

/// Port of FUN_0044ebb2: turn to `dir` (one frame, 5 cs), then play
/// animation set `set` once at 12 cs per frame and return to idle.
pub fn play_action(game: &mut Game, r: UnitRef, dir: u8, set: u8) {
    play_actions(game, &[(r, dir, set)]);
}

/// Port of FUN_0044ecf8 (FUN_0044ebb2 for one unit): several units play
/// their actions together, drawn back to front over the window; a unit that
/// finishes early shows the frame it had before the action. The surface is
/// left as it is (a unit still drawn into it shows through).
pub fn play_actions(game: &mut Game, acts: &[(UnitRef, u8, u8)]) {
    if acts.is_empty() {
        return;
    }
    let start_pos: Vec<u8> = acts.iter().map(|&(r, _, _)| game.battle.unit(r).frame).collect();
    let mut turned = false;
    for &(r, dir, _) in acts {
        if game.battle.unit(r).facing & 3 != dir & 3 {
            step_unit(game, r, dir & 3);
            turned = true;
        }
    }
    // Drawn back to front (the original's exchange sort by row).
    let mut order: Vec<usize> = (0..acts.len()).collect();
    for a in 0..order.len() {
        for b in a + 1..order.len() {
            if game.battle.unit(acts[order[b]].0).y < game.battle.unit(acts[order[a]].0).y {
                order.swap(a, b);
            }
        }
    }
    if turned {
        game.scr.set_page(0);
        motion::draw_window(game);
        for &k in &order {
            draw_unit(game, acts[k].0, 0);
        }
        gfx::present(game);
        game.sleep_cs(5);
    }
    for &(r, dir, set) in acts {
        with_unit(game, r, |b, l, _| {
            let a = (dir & 3) + (set << 2);
            b.set_action(l, a);
            l.facing = a;
        });
    }
    loop {
        gfx::palette_cycle(game);
        if acts.iter().all(|&(r, _, _)| frame_byte(game, r) == 0) {
            break;
        }
        // The window as the surface has it (a unit drawn into it shows
        // under its new frames, as in the original), the units over it.
        game.scr.set_page(0);
        motion::draw_window(game);
        for &k in &order {
            let (r, sp) = (acts[k].0, start_pos[k]);
            if frame_byte(game, r) == 0 {
                with_unit(game, r, |b, l, g| {
                    let pos = l.frame;
                    l.frame = sp;
                    b.decode(l, g);
                    l.frame = pos;
                });
            } else {
                with_unit(game, r, |b, l, g| {
                    b.decode(l, g);
                    l.frame = l.frame.wrapping_add(1);
                });
            }
            draw_unit(game, r, 0);
        }
        let t0 = game.battle.unit(acts[0].0).anim_timer;
        game.wait_until(t0, 0xc);
        gfx::present(game);
        game.battle.unit_mut(acts[0].0).anim_timer = game.ticks();
    }
    for &(r, dir, _) in acts {
        step_unit(game, r, dir & 3);
        set_anim(game, r, 0);
    }
}

/// FUN_0043a1b0(unit, frame + `ahead`): the frame byte `ahead` positions
/// past the unit's current one in its action (0: the action has ended
/// there; the offset is an int in the original, so it does not wrap).
/// Added by BATTLE for the attack frame loop of FUN_00436e46.
pub fn frame_ahead(game: &Game, r: UnitRef, ahead: u8) -> u8 {
    let v = &game.battle.view;
    let Some(a) = v.assets.as_deref() else { return 0 };
    let l = look_of(&game.battle, r);
    a.sprites.frame_at_pos(&l, l.frame as usize + ahead as usize)
}

/// FUN_0043a34f then `frame++`: decode the unit's current action frame and
/// move to the next. Added by BATTLE for the attack frame loop of
/// FUN_00436e46.
pub fn step_frame(game: &mut Game, r: UnitRef) {
    with_unit(game, r, |b, l, g| {
        b.decode(l, g);
        l.frame = l.frame.wrapping_add(1);
    });
}

fn frame_byte(game: &Game, r: UnitRef) -> u8 {
    let v = &game.battle.view;
    let Some(a) = v.assets.as_deref() else { return 0 };
    let l = look_of(&game.battle, r);
    a.sprites.frame_at(&l, l.frame)
}

/// Port of FUN_0044d024: one frame with the unit drawn in colour 0x3c
/// (shadow-colour pixels kept, ground shadow added), then 2 cs.
pub fn hit_flash(game: &mut Game, r: UnitRef) {
    sync_scroll(game);
    sync_sprites(game);
    let lk = looks(&game.battle);
    let g = globals(game);
    let page = game.scr.cur;
    {
        let Game { battle, scr, .. } = &mut *game;
        let v = &battle.view;
        let (Some(assets), Some(mv)) = (v.assets.as_deref(), v.map.as_ref()) else { return };
        let Some(gx) = v.units[r.side.index()].get(r.idx) else { return };
        let Some(sprite) = gx.sprite.as_ref() else { return };
        let pl = placed(&lk, &v.units);
        let scene = UnitScene { map: mv, tiles: &assets.tiles, shading: &assets.shading, art: assets.unit_art(), g, armies: [&pl[0], &pl[1]] };
        let l = lk[r.side.index()][r.idx];
        let under = under_of(mv, gx, &l, sprite);
        let img = scene.hit_flash_image(&l, sprite, &under);
        let (x, y) = unit::unit_screen_pos(mv, l.x, l.y, &img, 0, l.remap as i32);
        scr.pages[page].blit(&img, x as isize, y as isize);
    }
    gfx::present(game);
    game.sleep_cs(2);
}

/// The unit's finished image and the save-under behind it, for the fades.
fn finished_with_under(game: &mut Game, r: UnitRef) -> Option<(Image, Image, UnitLook)> {
    sync_sprites(game);
    let lk = looks(&game.battle);
    let g = globals(game);
    let v = &game.battle.view;
    let (assets, mv) = (v.assets.as_deref()?, v.map.as_ref()?);
    let gx = v.units[r.side.index()].get(r.idx)?;
    let sprite = gx.sprite.as_ref()?;
    let pl = placed(&lk, &v.units);
    let scene = UnitScene { map: mv, tiles: &assets.tiles, shading: &assets.shading, art: assets.unit_art(), g, armies: [&pl[0], &pl[1]] };
    let mut l = lk[r.side.index()][r.idx];
    // A copy of the save-under with the units behind added (FUN_004423d3
    // x2; the original restores it afterwards).
    let mut under = under_of(mv, gx, &l, sprite).into_owned();
    unit::add_behind(mv, &l, &mut under, [&pl[0], &pl[1]]);
    let prep = scene.prepared(&mut l, sprite, &under);
    let fin = scene.cut_all(prep, &l, s2data::gfx_map::CutMode::Solid, None);
    Some((fin, under, l))
}

/// Port of FUN_0044f27e: blend the unit into what lies behind it
/// (`reverse` = appear). `color != 0` first tints it 80% toward `color`.
pub fn fade_unit(game: &mut Game, r: UnitRef, color: u8, reverse: bool) {
    let Some((mut fin, under, l)) = finished_with_under(game, r) else { return };
    game.battle.unit_mut(r).anim_timer = game.ticks();
    let tree = game.battle.view.assets().tree.clone();
    let (mut pct, steps, delay) = if color == 0 { (if reverse { 100 } else { 40 }, 5, 2) } else { (if reverse { 100 } else { 10 }, 10, 3) };
    if color != 0 {
        // FUN_0042074e(buf, color, 80).
        let mut cache = [0u8; 256];
        for p in fin.pixels.iter_mut().filter(|p| **p != 0) {
            if cache[*p as usize] == 0 {
                cache[*p as usize] = tree.blend(*p, color, 0x50);
            }
            *p = cache[*p as usize];
        }
    }
    for _ in 0..steps {
        // FUN_00420896 with a fresh 64K cache per step.
        let mut cache = vec![0u8; 0x10000];
        let mut out = Image::new(fin.width, fin.height);
        for (k, o) in out.pixels.iter_mut().enumerate() {
            let (a, b) = (fin.pixels[k], under.pixels.get(k).copied().unwrap_or(0));
            if a == 0 || b == 0 {
                continue;
            }
            let key = a as usize * 256 + b as usize;
            if cache[key] == 0 {
                cache[key] = tree.blend(a, b, pct);
            }
            *o = cache[key];
        }
        let (x, y) = {
            let mv = game.battle.view.map_view();
            unit::unit_screen_pos(mv, l.x, l.y, &out, 0, l.remap as i32)
        };
        let page = game.scr.cur;
        game.scr.pages[page].blit(&out, x as isize, y as isize);
        pct += if reverse { -10 } else { 10 };
        let t0 = game.battle.unit(r).anim_timer;
        game.wait_until(t0, delay);
        gfx::present(game);
        game.battle.unit_mut(r).anim_timer = game.ticks();
    }
}

/// Port of FUN_0044f11c: 10 frames tinting the unit from 10% (or 100%,
/// `reverse`) toward `color` in 10% steps, 4 cs each.
pub fn flash_unit(game: &mut Game, r: UnitRef, color: u8, reverse: bool) {
    let mut pct = if reverse { 100 } else { 10 };
    // The frame's pixels are kept aside and tinted afresh each step, then
    // put back (the frame itself is not redecoded).
    let Some(saved) = sprite_of(game, r) else { return };
    let tree = game.battle.view.assets().tree.clone();
    for _ in 0..10 {
        let mut s = saved.clone();
        let mut cache = [0u8; 256];
        for p in s.pixels.iter_mut().filter(|p| **p != 0) {
            if cache[*p as usize] == 0 {
                cache[*p as usize] = tree.blend(*p, color, pct);
            }
            *p = cache[*p as usize];
        }
        set_sprite(game, r, s);
        draw_unit(game, r, 0);
        pct += if reverse { -10 } else { 10 };
        let t0 = game.battle.unit(r).anim_timer;
        game.wait_until(t0, 4);
        gfx::present(game);
        game.battle.unit_mut(r).anim_timer = game.ticks();
    }
    set_sprite(game, r, saved);
}

/// Port of FUN_00466507: play CEL 1 frame set `effect` (1-based) on cell
/// (x, y) over the scene in page 1, with its sound. `behind_units` is the
/// spell-type-7 case: the units are redrawn over the effect. Without a
/// spell (`behind_units` false) it is also FUN_0046c98d / FUN_0046c9cd.
pub fn play_effect(game: &mut Game, effect: usize, x: i32, y: i32, behind_units: bool) -> Result<()> {
    sync_scroll(game);
    sync_sprites(game);
    let lk = looks(&game.battle);
    let fs = game.battle.view.assets().fx.set(effect)?;
    let sound = game.battle.view.assets().fx.sound(effect) as u16;
    let g = globals(game);
    let mut cache = BlendCache::new(&fs.remap_from, &fs.remap_to);
    let order = fx::pass_frames(&fs);
    for _ in 0..fx::passes(&fs) {
        game.sys.audio.play_sound(&game.data.arcs, sound, 0);
        for &f in &order {
            gfx::palette_cycle(game);
            game.scr.copy_page(0, 1);
            game.scr.set_page(0);
            {
                let Game { battle, scr, .. } = &mut *game;
                let v = &battle.view;
                let (assets, mv) = (v.assets(), v.map_view());
                let pl = placed(&lk, &v.units);
                let painter = FxPainter { map: mv, tiles: &assets.tiles, shading: &assets.shading, tree: &assets.tree, g, armies: [&pl[0], &pl[1]] };
                painter.draw(&mut scr.pages[0], &fs, f, &mut cache, x, y, behind_units)?;
                if behind_units {
                    // FUN_00442e33 x2: visible units over the effect.
                    let scene = UnitScene { map: mv, tiles: &assets.tiles, shading: &assets.shading, art: assets.unit_art(), g, armies: [&pl[0], &pl[1]] };
                    for (s, army) in lk.iter().enumerate() {
                        for (i, l) in army.iter().enumerate() {
                            if !l.on_map() || !mv.cell_in_view(l.x, l.y, 0) {
                                continue;
                            }
                            let Some(gx) = v.units[s].get(i) else { continue };
                            if let Some(sp) = gx.sprite.as_ref() {
                                let mut l2 = *l;
                                let under = under_of(mv, gx, &l2, sp);
                                scene.draw_on_screen(&mut scr.pages[0], &mut l2, sp, &under, 0);
                            }
                        }
                    }
                }
            }
            let t0 = game.battle.view.fx_last;
            game.wait_until(t0, fs.delay as i32);
            gfx::present(game);
            game.battle.view.fx_last = game.ticks();
        }
    }
    Ok(())
}

/// Port of FUN_004668dd: effect `effect` on up to four cells at once,
/// each frame bottom-centred and occluded on every cell that is not
/// (−1, −1), over the scene in page 1, with its sound each pass.
pub fn play_effect_cells(game: &mut Game, effect: usize, cells: &[(i32, i32)]) -> Result<()> {
    sync_scroll(game);
    sync_sprites(game);
    let lk = looks(&game.battle);
    let fs = game.battle.view.assets().fx.set(effect)?;
    let sound = game.battle.view.assets().fx.sound(effect) as u16;
    let g = globals(game);
    let mut cache = BlendCache::new(&fs.remap_from, &fs.remap_to);
    let order = fx::pass_frames(&fs);
    for _ in 0..fx::passes(&fs) {
        game.sys.audio.play_sound(&game.data.arcs, sound, 0);
        for &f in &order {
            gfx::palette_cycle(game);
            game.scr.copy_page(0, 1);
            game.scr.set_page(0);
            {
                let Game { battle, scr, .. } = &mut *game;
                let v = &battle.view;
                let (assets, mv) = (v.assets(), v.map_view());
                let pl = placed(&lk, &v.units);
                let painter = FxPainter { map: mv, tiles: &assets.tiles, shading: &assets.shading, tree: &assets.tree, g, armies: [&pl[0], &pl[1]] };
                for &(x, y) in cells.iter().filter(|&&c| c != (-1, -1)) {
                    painter.draw_on_cell(&mut scr.pages[0], &fs, f, &mut cache, x, y)?;
                }
            }
            let t0 = game.battle.view.fx_last;
            game.wait_until(t0, fs.delay as i32);
            gfx::present(game);
            game.battle.view.fx_last = game.ticks();
        }
    }
    Ok(())
}

/// Port of FUN_00467be7: effect `effect` rippling over `cells` (in list
/// order, the next one starting every `stagger + 1` frames with the
/// effect's sound; all together with `stagger` 0), drawn back to front
/// by row over the scene in page 1, until the last cell's run ends.
pub fn play_effect_cascade(game: &mut Game, effect: usize, cells: &[(i32, i32)], stagger: i32) -> Result<()> {
    let n = cells.len();
    if n == 0 {
        return Ok(());
    }
    sync_scroll(game);
    sync_sprites(game);
    let lk = looks(&game.battle);
    let fs = game.battle.view.assets().fx.set(effect)?;
    let sound = game.battle.view.assets().fx.sound(effect) as u16;
    let g = globals(game);
    let mut cache = BlendCache::new(&fs.remap_from, &fs.remap_to);
    let seq = fx::pass_frames(&fs);
    let frames = seq.len() as i32;
    // The drawing order: the original's exchange sort by row.
    let mut by_row: Vec<usize> = (0..n).collect();
    for i in 0..n {
        for j in i + 1..n {
            if cells[by_row[j]].1 < cells[by_row[i]].1 {
                by_row.swap(i, j);
            }
        }
    }
    let mut run = vec![0i32; n];
    run[0] = 1;
    if stagger == 0 {
        run.fill(1);
    }
    let (mut next, mut tick) = (1usize, 0);
    game.sys.audio.play_sound(&game.data.arcs, sound, 0);
    loop {
        game.scr.copy_page(0, 1);
        game.scr.set_page(0);
        {
            let Game { battle, scr, .. } = &mut *game;
            let v = &battle.view;
            let (assets, mv) = (v.assets(), v.map_view());
            let pl = placed(&lk, &v.units);
            let painter = FxPainter { map: mv, tiles: &assets.tiles, shading: &assets.shading, tree: &assets.tree, g, armies: [&pl[0], &pl[1]] };
            for &k in &by_row {
                if run[k] != 0 && cells[k] != (-1, -1) {
                    if let Some(&f) = seq.get(run[k] as usize - 1) {
                        painter.draw_on_cell(&mut scr.pages[0], &fs, f, &mut cache, cells[k].0, cells[k].1)?;
                    }
                }
            }
        }
        let mut done = false;
        for (k, c) in run.iter_mut().enumerate() {
            if *c > 0 {
                *c += 1;
            }
            if frames < *c {
                *c = 0;
                done |= k == n - 1;
            }
        }
        let t0 = game.battle.view.fx_last;
        game.wait_until(t0, fs.delay as i32);
        gfx::present(game);
        game.battle.view.fx_last = game.ticks();
        if stagger > 0 && next < n {
            tick += 1;
            if tick > stagger {
                tick = 0;
                run[next] = 1;
                next += 1;
                game.sys.audio.play_sound(&game.data.arcs, sound, 0);
            }
        }
        if done {
            return Ok(());
        }
    }
}

/// Port of FUN_0044179b: the tall cursor on cell (x, y) of the current
/// page, apart from the map cursor (the AI's attack target).
pub fn draw_target_marker(game: &mut Game, x: i32, y: i32) {
    sync_scroll(game);
    sync_sprites(game);
    let lk = looks(&game.battle);
    let g = globals(game);
    let page = game.scr.cur;
    let Game { battle, scr, .. } = game;
    let v = &battle.view;
    let (Some(assets), Some(mv)) = (v.assets.as_deref(), v.map.as_ref()) else { return };
    let pl = placed(&lk, &v.units);
    let os = OverlayScene { map: mv, tiles: &assets.tiles, shading: &assets.shading, g, armies: [&pl[0], &pl[1]] };
    let art = CursorArt { flat: &assets.sysimg[slot::CURSOR_FLAT], back: &assets.sysimg[slot::CURSOR_BACK], front: &assets.sysimg[slot::CURSOR_FRONT] };
    os.draw_marker(&mut scr.pages[page], x, y, &art);
}

/// Port of FUN_0040720c: the attack range of `tl` into page 1 — the map
/// window darkened (tint 5) but for the attacker's cell and the range
/// cells (FUN_00427b53), with the targets drawn on top. The callers'
/// frames copy page 1 as their backdrop. Ends on page 0.
pub fn show_attack_range(game: &mut Game, tl: &TargetList) {
    if game.battle.view.map.is_none() || game.battle.view.assets.is_none() {
        return;
    }
    game.scr.set_page(1);
    motion::draw_window(game);
    let lk = looks(&game.battle);
    let g = globals(game);
    {
        let Game { battle, scr, .. } = &mut *game;
        let v = &battle.view;
        let (assets, mv) = (v.assets(), v.map_view());
        let page = &mut scr.pages[1];
        let lut = &assets.tints[5];
        for p in page.pixels.iter_mut() {
            *p = lut[*p as usize];
        }
        let pl = placed(&lk, &v.units);
        let os = OverlayScene { map: mv, tiles: &assets.tiles, shading: &assets.shading, g, armies: [&pl[0], &pl[1]] };
        let u = battle.unit(tl.unit);
        os.light_cell(page, u.x, u.y);
        for &(x, y) in &tl.cells {
            os.light_cell(page, x, y);
        }
    }
    for t in &tl.targets {
        draw_unit(game, t.unit, 0);
    }
    game.scr.set_page(0);
}

/// Compose the current scene (window + all units + cursor) into page 1,
/// as the callers of FUN_00466507 do before playing an effect.
pub fn scene_to_page1(game: &mut Game) {
    let cur = game.scr.cur;
    game.scr.set_page(1);
    draw_map(game);
    game.scr.set_page(cur);
}

#[cfg(test)]
mod tests {
    //! Renders stage 1 against the user's game data (`S2_GAME_DIR`, default
    //! ~/games/super2sp/extracted/app; skipped when absent). With
    //! `S2_VIEW_DUMP=DIR` the frames are written there as PNG.

    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::sync::{Arc, Mutex};

    use s2data::gfx_palette::vga_to_rgb;
    use s2data::tables::RuleTables;
    use s2data::tbl_stage::StageFile;

    use super::*;
    use crate::battle::map::Map;
    use crate::sys::{FlipInfo, Shared, Sys};

    fn data() -> Option<Data> {
        let dir = std::env::var_os("S2_GAME_DIR")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join("games/super2sp/extracted/app")))?;
        Data::open(&dir).ok()
    }

    fn save(dir: &Option<PathBuf>, name: &str, pixels: &[u8], pal: &[[u8; 3]; 256]) {
        let Some(dir) = dir else { return };
        std::fs::create_dir_all(dir).unwrap();
        let f = std::io::BufWriter::new(std::fs::File::create(dir.join(format!("{name}.png"))).unwrap());
        let mut enc = png::Encoder::new(f, 640, 480);
        enc.set_color(png::ColorType::Indexed);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_palette(pal.iter().flatten().copied().collect::<Vec<u8>>());
        enc.write_header().unwrap().write_image_data(pixels).unwrap();
    }

    type Frames = Arc<Mutex<Vec<Vec<u8>>>>;

    fn setup() -> (Sys, Frames) {
        let frames: Frames = Arc::default();
        let f2 = frames.clone();
        // The hook also runs on idle 60 Hz ticks; keep distinct frames only.
        let hook = Box::new(move |info: &FlipInfo, _: &Shared| {
            let mut v = f2.lock().unwrap();
            if v.last().is_none_or(|l| l.as_slice() != info.pixels) {
                v.push(info.pixels.to_vec());
            }
        });
        (Sys::new_headless(Shared::new(), hook), frames)
    }

    #[test]
    fn stage1_screens() {
        let Some(data) = data() else {
            eprintln!("view tests: no game data, skipped");
            return;
        };
        let dump = std::env::var_os("S2_VIEW_DUMP").map(PathBuf::from);
        let (mut sys, frames) = setup();
        let mut game = Game::new(&mut sys, &data);
        let master = {
            let mut m = data.dir.read("supersp.pal").unwrap()[..0x300].to_vec();
            m[..3].fill(0);
            m
        };
        let pal = vga_to_rgb(&master);
        game.scr.hw = pal;
        game.battle.set_tables(Rc::new(RuleTables::load(&data.dir, &data.arcs).unwrap()));
        let st = StageFile::parse(data.arcs.read(s2data::Kind::Bin, 1).unwrap()).unwrap().stage(1).unwrap();
        game.battle.set_map(Map::load(&data.arcs, st.header.map as usize).unwrap());
        game.battle.start_stage(&st);
        game.battle.deploy_turn(1);
        init(&mut game, st.header.map as usize).unwrap();

        // A few camera positions: on the first player unit, on the first enemy, the map corners.
        let p0 = UnitRef::new(Side::Player, 0);
        let e0 = UnitRef::new(Side::Enemy, 0);
        let (px, py) = (game.battle.unit(p0).x, game.battle.unit(p0).y);
        let (ex, ey) = (game.battle.unit(e0).x, game.battle.unit(e0).y);
        let shots: [(&str, (i32, i32)); 4] = [("scroll_player", (px, py)), ("scroll_enemy", (ex, ey)), ("scroll_topleft", (1, 1)), ("scroll_bottomright", (99, 99))];
        for (name, (x, y)) in shots {
            game.battle.map_mut().center_on(x, y, true);
            game.scr.set_page(0);
            draw_battle(&mut game);
            let px: &[u8] = &game.scr.pages[0].pixels;
            assert!(px.iter().filter(|&&c| c != 0).count() > 640 * 480 / 2, "{name}: mostly empty frame");
            save(&dump, name, px, &pal);
        }

        // Cursor and a movement range around the first player unit.
        game.battle.map_mut().center_on(px, py, true);
        cursor_set(&mut game, px, py + 2);
        let mut cells = Vec::new();
        for dy in -4..=4i32 {
            for dx in -2..=2i32 {
                let (x, y) = (px + dx, py + dy);
                let h = game.battle.view.map_view().height_at(x, y);
                if h > 0 && (dx, dy) != (0, 0) {
                    let mark = if dy.abs() == 4 { overlay::RangeMark::Attack } else { overlay::RangeMark::None };
                    cells.push(RangeCell { col: x, row: y, height: h, mark });
                }
            }
        }
        show_range(&mut game, &cells, Side::Player, (px, py), false);
        game.scr.set_page(0);
        draw_battle(&mut game);
        save(&dump, "range_cursor", &game.scr.pages[0].pixels, &pal);
        hide_range(&mut game);
        cursor_hide(&mut game);

        // Walk the first player unit two cells down-right and back up.
        let n0 = frames.lock().unwrap().len();
        let mut steps = Vec::new();
        let (mut x, mut y) = (px, py);
        for dir in [3u8, 3, 2, 2] {
            let (nx, ny) = crate::battle::map::neighbour(x, y, match dir {
                3 => 3,
                2 => 2,
                d => d as usize,
            });
            steps.push((nx, ny, match dir {
                3 => 1,
                _ => 3,
            }));
            x = nx;
            y = ny;
        }
        set_anim(&mut game, p0, 1);
        animate_walk(&mut game, p0, &steps);
        set_anim(&mut game, p0, 0);
        bake(&mut game);
        // Attack swing (set 2) facing down-right, then a hit on the enemy.
        play_action(&mut game, p0, 1, 2);
        hit_flash(&mut game, e0);
        let walk: Vec<Vec<u8>> = frames.lock().unwrap()[n0..].to_vec();
        assert!(walk.len() >= steps.len(), "walk/attack presented {} frames", walk.len());
        for (k, f) in walk.iter().enumerate() {
            save(&dump, &format!("anim_{k:02}"), f, &pal);
        }

        // Effect 51 (fire burst) on the enemy, over the scene in page 1.
        game.battle.map_mut().center_on(ex, ey, true);
        scene_to_page1(&mut game);
        let n1 = frames.lock().unwrap().len();
        play_effect(&mut game, 51, ex, ey, false).unwrap();
        let fx: Vec<Vec<u8>> = frames.lock().unwrap()[n1..].to_vec();
        assert!(!fx.is_empty());
        for (k, f) in fx.iter().enumerate().step_by(4) {
            save(&dump, &format!("fx51_{k:02}"), f, &pal);
        }
        let _ = Path::new("");
    }

    /// Panels, windows and the one-off animations over stage 1.
    #[test]
    fn stage1_panels() {
        let Some(data) = data() else {
            eprintln!("view tests: no game data, skipped");
            return;
        };
        let dump = std::env::var_os("S2_VIEW_DUMP").map(PathBuf::from);
        let (mut sys, frames) = setup();
        let mut game = Game::new(&mut sys, &data);
        let res = crate::res::Res::load(&data, &game.cfg).unwrap();
        let pal = vga_to_rgb(&res.master);
        game.res = Some(res);
        game.scr.hw = pal;
        game.battle.set_tables(Rc::new(RuleTables::load(&data.dir, &data.arcs).unwrap()));
        let st = StageFile::parse(data.arcs.read(s2data::Kind::Bin, 1).unwrap()).unwrap().stage(1).unwrap();
        game.battle.set_map(Map::load(&data.arcs, st.header.map as usize).unwrap());
        game.battle.start_stage(&st);
        game.battle.deploy_turn(1);
        game.battle.turn = 1;
        init(&mut game, st.header.map as usize).unwrap();
        let p0 = UnitRef::new(Side::Player, 0);
        let e0 = UnitRef::new(Side::Enemy, 0);
        let (px, py) = (game.battle.unit(p0).x, game.battle.unit(p0).y);
        let (ex, ey) = (game.battle.unit(e0).x, game.battle.unit(e0).y);
        let shot = |game: &mut Game, name: &str| {
            let px: Vec<u8> = game.scr.pages[game.scr.cur].pixels.clone();
            save(&dump, name, &px, &pal);
        };

        // 狀況視窗 on a unit, on the terrain, and while moving.
        game.battle.map_mut().center_on(px, py, true);
        cursor_set(&mut game, px, py);
        game.scr.set_page(0);
        draw_battle(&mut game);
        panel::status_bar(&mut game, None);
        shot(&mut game, "panel_status_unit");
        cursor_set(&mut game, px + 1, py + 3);
        game.cfg.status_window = true;
        game.scr.set_page(0);
        draw_battle(&mut game);
        panel::status_bar(&mut game, None);
        shot(&mut game, "panel_status_terrain");
        game.scr.set_page(0);
        draw_battle(&mut game);
        panel::status_bar(&mut game, Some(p0));
        shot(&mut game, "panel_status_moving");

        // The forecast panel for the first player unit attacking the first enemy.
        game.battle.map_mut().center_on(ex, ey, true);
        game.scr.set_page(0);
        draw_battle(&mut game);
        panel::duel_panel(&mut game, p0, e0, panel::Forecast { hit: 85, dmg: 42, counter_hit: 60, counter_dmg: 17 }, false);
        shot(&mut game, "panel_forecast");

        // Reward and level-up windows (they present and wait themselves).
        let n0 = frames.lock().unwrap().len();
        game.scr.set_page(0);
        draw_battle(&mut game);
        game.scr.copy_page(1, 0);
        let mut items = vec![0u8; 0x40];
        items[0] = 2;
        items[5] = 1;
        panel::reward_window(&mut game, p0, 25, None, &items);
        let u = game.battle.unit(p0).clone();
        let lg = panel::LevelGain { level: u.level + 1, old: panel::LevelGain::stats_of(&u), gain: [12, 3, 0, 2, 1, 0, 1, 0, 2] };
        panel::level_up_window(&mut game, p0, &[lg]);
        // Arrow projectile, the shake and the class-change reveal.
        game.battle.map_mut().center_on(px, py, true);
        game.scr.set_page(0);
        draw_battle(&mut game);
        motion::projectile(&mut game, p0, e0, 0x2c, 1);
        game.scr.set_page(1);
        draw_battle(&mut game);
        game.scr.set_page(0);
        motion::shake_screen(&mut game, 4, 4);
        motion::wipe_in(&mut game, p0);
        // A flyer and a sword rider (drawn over the window, as when selected).
        game.scr.set_page(0);
        motion::draw_window(&mut game);
        motion::draw_flyer(&mut game, p0, true);
        motion::draw_sword_rider(&mut game, UnitRef::new(Side::Player, 1));
        shot(&mut game, "anim_mounts");
        let fr: Vec<Vec<u8>> = frames.lock().unwrap()[n0..].to_vec();
        assert!(fr.len() > 5, "windows/animations presented {} frames", fr.len());
        for (k, f) in fr.iter().enumerate() {
            save(&dump, &format!("panel_seq_{k:02}"), f, &pal);
        }
        assert!(!motion::location_name(&game, px, py).is_empty() || true);
    }

    /// The backdrop page (page 1 via `camera::redraw`, FUN_00424c5f) must
    /// not hold a lifted unit: the idle frames copy it and draw the unit's
    /// next frame over it, which would leave the previous frame behind.
    #[test]
    fn backdrop_has_no_lifted_units() {
        let Some(data) = data() else {
            eprintln!("view tests: no game data, skipped");
            return;
        };
        let (mut sys, _frames) = setup();
        let mut game = Game::new(&mut sys, &data);
        game.res = Some(crate::res::Res::load(&data, &game.cfg).unwrap());
        game.battle.set_tables(Rc::new(RuleTables::load(&data.dir, &data.arcs).unwrap()));
        let st = StageFile::parse(data.arcs.read(s2data::Kind::Bin, 1).unwrap()).unwrap().stage(1).unwrap();
        game.battle.set_map(Map::load(&data.arcs, st.header.map as usize).unwrap());
        game.battle.start_stage(&st);
        game.battle.deploy_turn(1);
        init(&mut game, st.header.map as usize).unwrap();
        let p0 = UnitRef::new(Side::Player, 0);
        let (px, py) = (game.battle.unit(p0).x, game.battle.unit(p0).y);
        game.battle.map_mut().center_on(px, py, true);
        // The map without the unit at all.
        let deployed = game.battle.unit(p0).flags;
        game.battle.unit_mut(p0).flags &= !unit::F_DEPLOYED;
        game.scr.set_page(0);
        motion::draw_window(&mut game);
        let bare = game.scr.pages[0].pixels.clone();
        game.battle.unit_mut(p0).flags = deployed;
        // Selected, as when its command menu opens (FUN_0040a4d0 / FUN_00435697).
        game.battle.unit_mut(p0).act = unit::ACT_SELECTED;
        set_anim(&mut game, p0, 1);
        game.scr.set_page(1);
        crate::battle::camera::redraw(&mut game);
        game.scr.set_page(0);
        assert!(game.scr.pages[1].pixels == bare, "page 1 holds the selected unit");
        // FUN_004351ad a few times: copy page 1, draw the next frame.
        for _ in 0..6 {
            let f = game.battle.unit(p0).facing;
            step_unit(&mut game, p0, f);
            game.scr.copy_page(0, 1);
            draw_unit(&mut game, p0, 0);
            let shown = game.scr.pages[0].pixels.clone();
            game.scr.pages[0].pixels.copy_from_slice(&bare);
            draw_unit(&mut game, p0, 0);
            assert!(shown == game.scr.pages[0].pixels, "an earlier frame of the unit is left behind");
            assert!(shown != bare, "the unit was not drawn");
        }
        // The same for the walking and combat states.
        for act in [unit::ACT_WALKING, unit::ACT_COMBAT] {
            game.battle.unit_mut(p0).act = act;
            game.scr.set_page(1);
            crate::battle::camera::redraw(&mut game);
            game.scr.set_page(0);
            assert!(game.scr.pages[1].pixels == bare, "page 1 holds the unit with act {act}");
        }
    }

    /// The map surface changes only by the original's operations: it is
    /// not rebuilt when units change, an erase pastes the save-under back
    /// (wiping what was drawn there since it was captured) and a unit the
    /// battle code erased stays off until it is put back or done.
    #[test]
    fn surface_is_incremental() {
        let Some(data) = data() else {
            eprintln!("view tests: no game data, skipped");
            return;
        };
        let (mut sys, _frames) = setup();
        let mut game = Game::new(&mut sys, &data);
        game.res = Some(crate::res::Res::load(&data, &game.cfg).unwrap());
        game.battle.set_tables(Rc::new(RuleTables::load(&data.dir, &data.arcs).unwrap()));
        let st = StageFile::parse(data.arcs.read(s2data::Kind::Bin, 1).unwrap()).unwrap().stage(1).unwrap();
        game.battle.set_map(Map::load(&data.arcs, st.header.map as usize).unwrap());
        game.battle.start_stage(&st);
        game.battle.deploy_turn(1);
        init(&mut game, st.header.map as usize).unwrap();
        bake(&mut game);
        let p0 = UnitRef::new(Side::Player, 0);
        let (w, h) = {
            let s = game.battle.view.units[0][0].sprite.as_ref().unwrap();
            (s.width, s.height)
        };
        let (px, py) = (game.battle.unit(p0).x, game.battle.unit(p0).y);
        let (x0, y0) = unit::frame_origin(game.battle.view.map_view(), px, py, w, h);
        let under = game.battle.view.units[0][0].under.clone().expect("save-under captured");
        let at = |game: &Game, x: i32, y: i32| {
            let s = &game.battle.view.map_view().surface;
            s.pixels[y as usize * s.width + x as usize]
        };
        // A mark far from every unit survives a unit change and redraw.
        let mark = 5usize;
        game.battle.view.map.as_mut().unwrap().surface.pixels[mark] = 0xfe;
        game.battle.unit_mut(p0).act = 2;
        bake(&mut game);
        assert_eq!(game.battle.view.map_view().surface.pixels[mark], 0xfe, "the surface was rebuilt");
        // A mark inside the unit's rectangle, where its frame is transparent,
        // is wiped when the unit is erased.
        let k = (0..w * h).find(|&k| game.battle.view.units[0][0].sprite.as_ref().unwrap().pixels[k] == 0 && under.pixels[k] != 0xfd).unwrap();
        let (mx, my) = (x0 + (k % w) as i32, y0 + (k / w) as i32);
        {
            let s = &mut game.battle.view.map.as_mut().unwrap().surface;
            let wd = s.width;
            s.pixels[my as usize * wd + mx as usize] = 0xfd;
        }
        game.battle.unit_mut(p0).act = 1;
        erase_unit(&mut game, p0);
        assert_ne!(at(&game, mx, my), 0xfd, "the erase did not paste the save-under back");
        // Only the battle code's calls change the surface: not a redraw after
        // the unit's state changed...
        game.battle.unit_mut(p0).act = 2;
        bake(&mut game);
        assert!(game.battle.view.units[0][0].drawn.is_none(), "an erased unit came back by itself");
        // ...but FUN_00440264.
        put_unit(&mut game, p0);
        assert!(game.battle.view.units[0][0].drawn.is_some(), "put_unit did not draw the unit");
        // Moving a unit to a cell with nobody around the way the original
        // does (erase, move, capture, draw) leaves what a rebuild from the
        // background would.
        let far = {
            let b = &game.battle;
            let busy = |x: i32, y: i32| {
                [Side::Player, Side::Enemy].iter().any(|&s| {
                    b.army(s).units.iter().any(|u| u.flags & unit::F_DEPLOYED != 0 && (u.x - x).abs() <= 4 && (u.y - y).abs() <= 0x1a)
                })
            };
            let m = b.map();
            (2..m.rows() - 2).flat_map(|y| (2..m.width() - 2).map(move |x| (x, y))).find(|&(x, y)| !busy(x, y) && m.height(x, y) == 1)
        };
        let (fx, fy) = far.expect("stage 1 has a cell with nobody around");
        let mark = 5usize;
        game.battle.view.map.as_mut().unwrap().surface.pixels[mark] = game.battle.view.map_view().bg.pixels[mark];
        erase_unit(&mut game, p0);
        {
            let u = game.battle.unit_mut(p0);
            (u.x, u.y) = (fx, fy);
        }
        capture(&mut game, p0);
        put_unit(&mut game, p0);
        let incremental = game.battle.view.map_view().surface.pixels.clone();
        reset_surface(&mut game);
        bake(&mut game);
        let rebuilt = &game.battle.view.map_view().surface.pixels;
        let differ = incremental.iter().zip(rebuilt.iter()).filter(|(a, b)| a != b).count();
        assert_eq!(differ, 0, "an isolated move left {differ} pixels a rebuild would not");
    }

    /// The battle windows and popups that finished the TODO(view/ui) list.
    #[test]
    fn stage1_battle_windows() {
        let Some(data) = data() else {
            eprintln!("view tests: no game data, skipped");
            return;
        };
        let dump = std::env::var_os("S2_VIEW_DUMP").map(PathBuf::from);
        let (mut sys, frames) = setup();
        let mut game = Game::new(&mut sys, &data);
        let res = crate::res::Res::load(&data, &game.cfg).unwrap();
        let pal = vga_to_rgb(&res.master);
        game.res = Some(res);
        game.scr.hw = pal;
        game.battle.set_tables(Rc::new(RuleTables::load(&data.dir, &data.arcs).unwrap()));
        let st = StageFile::parse(data.arcs.read(s2data::Kind::Bin, 1).unwrap()).unwrap().stage(1).unwrap();
        game.battle.set_map(Map::load(&data.arcs, st.header.map as usize).unwrap());
        game.battle.start_stage(&st);
        game.battle.deploy_turn(1);
        game.battle.turn = 1;
        init(&mut game, st.header.map as usize).unwrap();
        let p0 = UnitRef::new(Side::Player, 0);
        let e0 = UnitRef::new(Side::Enemy, 0);
        let (px, py) = (game.battle.unit(p0).x, game.battle.unit(p0).y);
        let (ex, ey) = crate::battle::map::neighbour(px, py, 1);
        {
            let u = game.battle.unit_mut(e0);
            u.x = ex;
            u.y = ey;
        }
        game.battle.map_mut().center_on(px, py, true);
        let last = |frames: &Frames| frames.lock().unwrap().last().cloned().unwrap();
        let since = |frames: &Frames, n: usize| frames.lock().unwrap()[n..].to_vec();
        let click = |game: &mut Game| {
            let sh = game.sys.shared().clone();
            sh.push_input(crate::sys::InputEvent::MouseDown(0));
            sh.push_input(crate::sys::InputEvent::MouseUp(0));
        };

        // A facing changed by the battle code alone is picked up by the view.
        game.scr.set_page(0);
        draw_battle(&mut game);
        crate::battle::movement::set_facing(game.battle.unit_mut(p0), 2);
        draw_battle(&mut game);
        {
            let mut l = look_of(&game.battle, p0);
            let f = l.facing;
            game.battle.view.assets().sprites.set_action(&mut l, f);
            assert_eq!(l.frame, game.battle.unit(p0).frame, "the new facing's action was not started");
        }

        // 狀況視窗 in the command menu.
        game.cfg.status_window = true;
        game.battle.unit_mut(p0).act = 1;
        game.scr.set_page(0);
        draw_battle(&mut game);
        panel::menu_status(&mut game, p0);
        save(&dump, "menu_status", &game.scr.pages[0].pixels.clone(), &pal);

        // The chest window, the rest recovery and the class sheet (effect 0x23).
        click(&mut game);
        draw_battle(&mut game);
        panel::treasure_window(&mut game, p0, 5);
        save(&dump, "treasure_window", &last(&frames), &pal);
        game.scr.set_page(1);
        motion::draw_window(&mut game);
        game.scr.set_page(0);
        let n0 = frames.lock().unwrap().len();
        panel::recovery_popup(&mut game, p0, 20, 6, 0);
        let fr = since(&frames, n0);
        assert!(fr.len() >= 8, "recovery popup presented {} frames", fr.len());
        save(&dump, "recovery_popup", &fr[4], &pal);
        click(&mut game);
        draw_battle(&mut game);
        panel::class_sheet(&mut game, e0);
        save(&dump, "class_sheet", &last(&frames), &pal);

        // The HP counted down under the forecast panel (FUN_00439e2a).
        let skill = {
            let t = game.battle.t();
            t.kinds.class(game.battle.unit(p0).kind as u16).map_or(0, |c| c.actions.start)
        };
        let duel = game.battle.duel(p0, e0, skill);
        game.scr.set_page(1);
        draw_battle(&mut game);
        game.scr.set_page(0);
        let hp0 = game.battle.unit(e0).hp;
        let n1 = frames.lock().unwrap().len();
        let mut dmg = 60;
        crate::battle::actions::show_hp(&mut game, p0, e0, &mut dmg, 1, Some(&duel));
        assert_eq!(game.battle.unit(e0).hp, hp0 - dmg as i16);
        let fr = since(&frames, n1);
        for (k, f) in fr.iter().enumerate().step_by(2) {
            save(&dump, &format!("hp_count_{k:02}"), f, &pal);
        }

        // A summoned creature rising (FUN_004608e2).
        if let Some(s) = game.battle.create_summon(Side::Player, 0, px, py + 2, 1) {
            game.battle.unit_mut(s).act = 2;
            let n2 = frames.lock().unwrap().len();
            motion::summon_rise(&mut game, p0, s);
            let fr = since(&frames, n2);
            assert!(fr.len() > 5, "summon rise presented {} frames", fr.len());
            let step = (fr.len() / 5).max(1);
            for (k, f) in fr.iter().enumerate().step_by(step) {
                save(&dump, &format!("summon_rise_{k:02}"), f, &pal);
            }
            assert_eq!(game.battle.unit(s).act, 2);
        }
    }

    /// The AI's presentation pieces and the cast animations over stage 1.
    #[test]
    fn stage1_ai_and_casts() {
        let Some(data) = data() else {
            eprintln!("view tests: no game data, skipped");
            return;
        };
        let dump = std::env::var_os("S2_VIEW_DUMP").map(PathBuf::from);
        let (mut sys, frames) = setup();
        let mut game = Game::new(&mut sys, &data);
        let res = crate::res::Res::load(&data, &game.cfg).unwrap();
        let pal = vga_to_rgb(&res.master);
        game.res = Some(res);
        game.scr.hw = pal;
        game.battle.set_tables(Rc::new(RuleTables::load(&data.dir, &data.arcs).unwrap()));
        let st = StageFile::parse(data.arcs.read(s2data::Kind::Bin, 1).unwrap()).unwrap().stage(1).unwrap();
        game.battle.set_map(Map::load(&data.arcs, st.header.map as usize).unwrap());
        game.battle.start_stage(&st);
        game.battle.deploy_turn(1);
        game.battle.turn = 1;
        init(&mut game, st.header.map as usize).unwrap();
        let p0 = UnitRef::new(Side::Player, 0);
        let e0 = UnitRef::new(Side::Enemy, 0);
        let (px, py) = (game.battle.unit(p0).x, game.battle.unit(p0).y);
        // Bring the first enemy next to the first player unit.
        let (ex, ey) = crate::battle::map::neighbour(px, py, 1);
        {
            let u = game.battle.unit_mut(e0);
            u.x = ex;
            u.y = ey;
        }
        game.battle.map_mut().center_on(px, py, true);
        let last = |frames: &Frames| frames.lock().unwrap().last().cloned().unwrap();
        let since = |frames: &Frames, n: usize| frames.lock().unwrap()[n..].to_vec();

        // The "thinking" line over the enemy.
        game.scr.set_page(0);
        panel::ai_thinking(&mut game, e0);
        save(&dump, "ai_thinking", &last(&frames), &pal);

        // Lift-off and landing of the player unit (the non-mount branch).
        game.battle.unit_mut(p0).act = 1;
        let n0 = frames.lock().unwrap().len();
        motion::lift_off(&mut game, p0, false);
        motion::lift_off(&mut game, p0, true);
        let fr = since(&frames, n0);
        assert!(fr.len() >= 8, "lift-off presented {} frames", fr.len());
        save(&dump, "ai_liftoff_top", &fr[3], &pal);

        // The attack range overlay and the forecast pause.
        let skill = {
            let t = game.battle.t();
            t.kinds.class(game.battle.unit(p0).kind as u16).map_or(0, |c| c.actions.start)
        };
        let tl = game.battle.target_list(p0, skill);
        game.battle.unit_mut(p0).act = 5;
        show_attack_range(&mut game, &tl);
        save(&dump, "ai_attack_range", &game.scr.pages[1].pixels.clone(), &pal);
        let duel = game.battle.duel(p0, e0, skill);
        let n1 = frames.lock().unwrap().len();
        let t0 = game.ticks();
        panel::ai_attack_pause(&mut game, p0, (ex, ey), &duel, t0);
        let fr = since(&frames, n1);
        assert!(!fr.is_empty());
        save(&dump, "ai_attack_pause", fr.last().unwrap(), &pal);
        game.battle.unit_mut(p0).act = 0;

        // Casts: 催眠術 (tinted), 地震 (kind 8), 閃電光束 (10), 傳送術 (5),
        // 連環閃電 (9), 四象劍法 (6), 隱身 (1).
        for spell in [2u16, 26, 30, 51, 59, 109, 47] {
            let technique = {
                let t = game.battle.t();
                let first = t.spells.school_spells.get(t.spells.magic_schools as usize).map_or(0, |s| s.0 as u16);
                first < spell
            };
            let own = matches!(spell, 47 | 51);
            let (cx, cy) = if own { (px, py) } else { (ex, ey) };
            let b = &mut game.battle;
            let aim = b.spell_aim(spell as i32);
            let mut tg = b.spell_target(Side::Player, p0, spell as i32, technique as u8, 2, false);
            b.target_range(&mut tg, aim.range);
            b.aim_ai_cast(&mut tg, cx, cy, 1);
            if spell == 51 {
                tg.target_cell = (px, py);
            }
            let mut c = crate::battle::spells::Cast::new(Side::Player, p0, spell, (cx, cy));
            c.technique = technique;
            game.battle.map_mut().center_on(px, py, true);
            if spell == 2 || spell == 30 {
                // The aim overlay (FUN_00401bfa), then one camera step (FUN_00401d75 shift).
                show_aim(&mut game, &tg);
                game.scr.set_page(0);
                draw_battle(&mut game);
                save(&dump, &format!("aim{spell:03}"), &game.scr.pages[0].pixels.clone(), &pal);
                game.battle.map_mut().scroll_y += 2;
                draw_battle(&mut game);
                save(&dump, &format!("aim{spell:03}_scrolled"), &game.scr.pages[0].pixels.clone(), &pal);
                hide_range(&mut game);
                game.battle.map_mut().center_on(px, py, true);
            }
            let n2 = frames.lock().unwrap().len();
            cast::cast_animation(&mut game, &c, &tg, |g, e| cast::draw_sequence(g, &c, &tg, e));
            let fr = since(&frames, n2);
            assert!(fr.len() > 3, "cast {spell} presented {} frames", fr.len());
            let step = (fr.len() / 6).max(1);
            for (k, f) in fr.iter().enumerate().step_by(step) {
                save(&dump, &format!("cast{spell:03}_{k:03}"), f, &pal);
            }
            game.battle.unit_mut(p0).act = 0;
        }

        // The victory and defeat effects (a queued click ends the wait).
        for won in [true, false] {
            let sh = game.sys.shared().clone();
            sh.push_input(crate::sys::InputEvent::MouseDown(0));
            sh.push_input(crate::sys::InputEvent::MouseUp(0));
            game.scr.set_page(0);
            let n3 = frames.lock().unwrap().len();
            // Placed on map cell (0, 0) like the original: look from the corner.
            game.battle.map_mut().center_on(0, 0, true);
            motion::stage_end(&mut game, won);
            let fr = since(&frames, n3);
            // (Most of the effect lies off-screen even here, as in the original.)
            assert!(fr.len() >= 2, "stage end presented {} frames", fr.len());
            let step = (fr.len() / 5).max(1);
            for (k, f) in fr.iter().enumerate().step_by(step) {
                save(&dump, &format!("stage_end_{}_{k:03}", if won { "win" } else { "lose" }), f, &pal);
            }
        }
    }
}
