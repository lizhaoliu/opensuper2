//! The map surface's unit bookkeeping (`DAT_004947f0`), ported from
//! `FUN_00442022` (save-under capture), `FUN_00440264` (draw one unit into
//! the surface), `FUN_00441c0b` (erase one: paste its save-under back and
//! redraw its neighbours), `FUN_004403b5` (draw an army), `FUN_00441222`
//! (erase an army) and `FUN_00441ef1` (capture an army).
//!
//! The surface is never rebuilt from the units: like the original, it only
//! changes by these operations, so a save-under captured at another moment
//! than the unit's drawing (or pasted over a neighbour the redraw does not
//! reach) leaves the same traces as in the original. The battle and event
//! code calls the operations where the original does (see
//! `docs/view-port-status.md`); nothing else touches the surface. With
//! `S2_BATTLE_TRACE`, [`Surf::reconcile`] reports images the original would
//! not have left (a debug check); `S2_SURFACE_AUTO=1` makes it fix them
//! instead (the earlier approximation, for comparing).

use s2data::sprite::Image;

use super::map::MapView;
use super::unit::{self, Drawn, DrawGlobals, Placed, UnitGfx, UnitLook, UnitScene, ACT_DONE, F_DEAD};
use super::ViewAssets;

/// `S2_SURFACE_AUTO=1`: let [`Surf::reconcile`] fix the surface where the
/// battle code made no explicit call (the earlier approximation, for
/// comparing); otherwise it only reports, with `S2_BATTLE_TRACE`.
pub fn auto_mode() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("S2_SURFACE_AUTO").is_some())
}

/// `+0x30` bit set (with `F_DEAD`) while a unit is erased (`0x22000000`).
const F_ERASING: u32 = 0x0200_0000;

/// The pieces of the view the operations work on, split for borrowing.
pub(super) struct Surf<'a> {
    pub assets: &'a ViewAssets,
    pub mv: &'a mut MapView,
    pub units: &'a mut [Vec<UnitGfx>; 2],
    /// Both armies' unit state (the battle code's).
    pub looks: [Vec<UnitLook>; 2],
    pub g: DrawGlobals,
}

/// A change [`reconcile`] makes for one unit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Fix {
    Keep,
    /// Draw it over its old image (same place and frame).
    Over,
    Erase,
    /// Erase it where it was, then draw it where it is.
    Move,
    Put,
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

impl Surf<'_> {
    fn sprite_dims(&self, s: usize, i: usize) -> Option<(usize, usize)> {
        self.units[s].get(i).and_then(|g| g.sprite.as_ref()).map(|p| (p.width, p.height))
    }

    /// Port of FUN_00442022(army, unit, mode).
    pub fn capture(&mut self, s: usize, i: usize, mode: i32) {
        let Some((w, h)) = self.sprite_dims(s, i) else { return };
        let mut buf = match self.units[s][i].under.take() {
            Some(u) if (u.width, u.height) == (w, h) => u,
            _ => Image::new(w, h),
        };
        let look = self.looks[s][i];
        {
            let pl = placed(&self.looks, self.units);
            unit::capture(self.mv, &self.mv.surface, &look, &mut buf, mode, [&pl[0], &pl[1]]);
        }
        crate::battle::trace!(
            "surface: capture {s}/{i} mode {mode} on {:?}{}",
            (look.x, look.y),
            if self.units[s][i].drawn.is_some() { " OVER ITS OWN IMAGE" } else { "" }
        );
        let g = &mut self.units[s][i];
        g.under = Some(buf);
        g.under_at = Some((look.x, look.y));
    }

    /// Port of FUN_00441ef1's body for one unit: capture without the
    /// neighbours' save-unders.
    fn grab(&mut self, s: usize, i: usize) {
        if !self.looks[s][i].on_map() {
            return;
        }
        self.capture(s, i, 1);
    }

    /// Port of FUN_00440264(army, unit, full): draw the unit into the
    /// surface with its save-under (for the shadow) and, when `full`, cut
    /// by scenery and the units in front. Returns the unit's render flags
    /// after drawing (the hit shake phase toggles).
    pub fn put(&mut self, s: usize, i: usize, full: bool) -> Option<u32> {
        let mut look = self.looks[s][i];
        if !look.on_map() {
            return None;
        }
        let mut surface = std::mem::replace(&mut self.mv.surface, Image::new(0, 0));
        let drawn = {
            let gfx = &self.units[s][i];
            let sprite = gfx.sprite.as_ref();
            sprite.map(|sprite| {
                let bare;
                let under = match gfx.under.as_ref() {
                    Some(u) => u,
                    None => {
                        bare = unit::bare_under(self.mv, &look, sprite);
                        &bare
                    }
                };
                let pl = placed(&self.looks, self.units);
                let scene = UnitScene {
                    map: self.mv,
                    tiles: &self.assets.tiles,
                    shading: &self.assets.shading,
                    art: self.assets.unit_art(),
                    g: self.g,
                    armies: [&pl[0], &pl[1]],
                };
                scene.put(&mut surface, &mut look, sprite, under, full);
                Drawn { look, w: sprite.width, h: sprite.height }
            })
        };
        self.mv.surface = surface;
        let d = drawn?;
        crate::battle::trace!("surface: put {s}/{i} on {:?} act {} facing {:#x} frame {} full {full}", (d.look.x, d.look.y), d.look.act, d.look.facing, d.look.frame);
        self.looks[s][i].render_flags = d.look.render_flags;
        self.units[s][i].drawn = Some(d);
        Some(d.look.render_flags)
    }

    /// Paste the unit's save-under back where it was drawn (or stands).
    fn paste_under(&mut self, s: usize, i: usize, at: (i32, i32, usize, usize)) {
        let Some(u) = self.units[s][i].under.as_ref() else { return };
        crate::battle::trace!("surface: paste {s}/{i} at {:?} (captured on {:?})", (at.0, at.1), self.units[s][i].under_at);
        let (x, y) = unit::frame_origin(self.mv, at.0, at.1, at.2, at.3);
        self.mv.surface.blit_opaque(u, x as isize, y as isize);
    }

    /// Port of FUN_00441c0b: erase the unit from the surface (its
    /// save-under pasted where it stands, `drawn` giving where that was)
    /// and redraw the units around it (FUN_00441b15 for its army, then the
    /// other) except those in `skip`.
    pub fn erase(&mut self, s: usize, i: usize, skip: &[(usize, usize)]) {
        let Some((w, h)) = self.sprite_dims(s, i) else { return };
        let at = match self.units[s][i].drawn {
            Some(d) => (d.look.x, d.look.y, d.w, d.h),
            None => (self.looks[s][i].x, self.looks[s][i].y, w, h),
        };
        let saved = self.looks[s][i].flags;
        self.looks[s][i].flags |= F_DEAD | F_ERASING;
        self.paste_under(s, i, at);
        self.units[s][i].drawn = None;
        for side in [s, 1 - s] {
            for k in 0..self.looks[side].len() {
                let o = self.looks[side][k];
                if self.units[side].get(k).is_none_or(|g| g.sprite.is_none()) || !o.is_baked() || skip.contains(&(side, k)) {
                    continue;
                }
                if (o.x, o.y) == (at.0, at.1) || o.x < at.0 - 2 || o.x > at.0 + 2 || o.y > at.1 + 0x18 || o.y < at.1 - 0x18 {
                    continue;
                }
                self.put(side, k, true);
            }
        }
        self.looks[s][i].flags = saved;
    }

    /// Port of FUN_004403b5(army): draw every unit of the army that is not
    /// lifted (act 1/3/5) into the surface, in unit order, over what is there.
    pub fn bake_army(&mut self, s: usize) {
        for i in 0..self.looks[s].len() {
            if self.looks[s][i].is_baked() && self.sprite_dims(s, i).is_some() {
                self.put(s, i, true);
            }
        }
    }

    /// Port of FUN_00441222(army): paste every unit's save-under back
    /// (no redraws).
    pub fn erase_army(&mut self, s: usize) {
        for i in 0..self.looks[s].len() {
            let l = self.looks[s][i];
            if !l.on_map() {
                // A unit that left the map still had its image in the
                // surface: the original erased it when it left.
                if let Some(d) = self.units[s][i].drawn {
                    self.paste_under(s, i, (d.look.x, d.look.y, d.w, d.h));
                    self.units[s][i].drawn = None;
                }
                continue;
            }
            let Some((w, h)) = self.sprite_dims(s, i) else { continue };
            let at = match self.units[s][i].drawn {
                Some(d) => (d.look.x, d.look.y, d.w, d.h),
                None => (l.x, l.y, w, h),
            };
            self.paste_under(s, i, at);
            self.units[s][i].drawn = None;
        }
    }

    /// Port of FUN_00441ef1(army): capture every unit's save-under as the
    /// surface is (no neighbours pasted).
    pub fn grab_army(&mut self, s: usize) {
        for i in 0..self.looks[s].len() {
            self.grab(s, i);
        }
    }

    /// Start over from the bare background (FUN_00424d81), the save-unders
    /// captured from it and both armies drawn (FUN_004433de x2 +
    /// FUN_004403b5 x2, as after loading a save).
    pub fn rebuild(&mut self) {
        self.mv.surface = self.mv.bg.clone();
        for s in 0..2 {
            for g in self.units[s].iter_mut() {
                g.drawn = None;
                g.held = false;
            }
        }
        self.grab_army(0);
        self.grab_army(1);
        self.bake_army(0);
        self.bake_army(1);
    }

    fn fix_for(&self, s: usize, i: usize) -> Fix {
        let l = self.looks[s][i];
        let g = &self.units[s][i];
        let Some(sp) = g.sprite.as_ref() else {
            return if g.drawn.is_some() { Fix::Erase } else { Fix::Keep };
        };
        // A unit the battle code took off the surface is its business until
        // it puts the unit back or the unit has finished (`act` 2): neither
        // drawn back when the battle code clears `act` for a moment
        // (FUN_00436e46 around the hit flash) nor erased when a redraw of its
        // neighbours drew it (FUN_00437b9c), unless it moved or left the map.
        if g.held && l.act != ACT_DONE {
            let gone = g.drawn.is_some_and(|d| {
                !l.on_map() || (d.look.x, d.look.y, d.w, d.h) != (l.x, l.y, sp.width, sp.height)
            });
            return if gone { Fix::Erase } else { Fix::Keep };
        }
        let want = l.is_baked();
        match g.drawn {
            None if want => Fix::Put,
            None => Fix::Keep,
            Some(d) => {
                let moved = (d.look.x, d.look.y, d.w, d.h) != (l.x, l.y, sp.width, sp.height)
                    || unit::frame_key(&d.look) != unit::frame_key(&l);
                if !l.on_map() {
                    Fix::Erase
                } else if !want {
                    // Drawn while lifted (the battle code put it there
                    // itself, as FUN_0044e210 does): left alone until it
                    // moves; lifted after being drawn: erased.
                    if d.look.is_baked() || moved {
                        Fix::Erase
                    } else {
                        Fix::Keep
                    }
                } else if moved {
                    Fix::Move
                } else if d.look != l {
                    Fix::Over
                } else {
                    Fix::Keep
                }
            }
        }
    }

    fn fixes(&self) -> Vec<(usize, usize, Fix)> {
        let mut fixes = Vec::new();
        for s in 0..2 {
            for i in 0..self.looks[s].len().min(self.units[s].len()) {
                let f = self.fix_for(s, i);
                if f != Fix::Keep {
                    fixes.push((s, i, f));
                }
            }
        }
        fixes
    }

    /// The erasing half of [`reconcile`](Self::reconcile) (all units but
    /// `except`): images units left behind go before an operation the
    /// battle code asks for, as the original erased them when they left.
    pub fn settle(&mut self, except: Option<(usize, usize)>) {
        if !auto_mode() {
            return;
        }
        let fixes = self.fixes();
        let pending: Vec<(usize, usize)> = fixes.iter().filter(|f| matches!(f.2, Fix::Move | Fix::Put)).map(|f| (f.0, f.1)).collect();
        for &(s, i, f) in &fixes {
            if matches!(f, Fix::Erase | Fix::Move) && except != Some((s, i)) {
                self.erase(s, i, &pending);
            }
        }
    }

    /// Bring the surface in line with the units: erase the units that left
    /// their image or were lifted (FUN_00441c0b), then capture the save-under
    /// of each unit that landed on a new cell (FUN_00442022) and draw the
    /// changed ones (FUN_00440264).
    pub fn reconcile(&mut self) {
        if !auto_mode() {
            return self.audit();
        }
        for s in 0..2 {
            for i in 0..self.looks[s].len().min(self.units[s].len()) {
                if self.looks[s][i].act == ACT_DONE {
                    self.units[s][i].held = false;
                }
            }
        }
        self.settle(None);
        for (s, i, f) in self.fixes() {
            if !matches!(f, Fix::Put | Fix::Over) {
                continue;
            }
            let l = self.looks[s][i];
            let fresh = self.units[s][i].under.is_none() || self.units[s][i].under_at != Some((l.x, l.y));
            if fresh {
                self.capture(s, i, 0);
            }
            self.put(s, i, true);
        }
    }

    /// Report (`S2_BATTLE_TRACE`) units whose surface image the original
    /// would not have left: dead or withdrawn units still drawn, units drawn
    /// on a cell they left.
    fn audit(&self) {
        if !crate::battle::tracing() {
            return;
        }
        for s in 0..2 {
            for i in 0..self.looks[s].len().min(self.units[s].len()) {
                let (l, g) = (&self.looks[s][i], &self.units[s][i]);
                let Some(d) = g.drawn else { continue };
                if !l.on_map() {
                    crate::battle::trace!("surface audit: {s}/{i} left the map but is still drawn on {:?}", (d.look.x, d.look.y));
                } else if (d.look.x, d.look.y) != (l.x, l.y) {
                    crate::battle::trace!("surface audit: {s}/{i} on {:?} still drawn on {:?}", (l.x, l.y), (d.look.x, d.look.y));
                }
            }
        }
    }
}
