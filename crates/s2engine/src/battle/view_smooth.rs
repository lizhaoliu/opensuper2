//! Smooth motion (not in the original; `Shared::smooth`, off by default):
//! in-between frames for unit moves and camera scrolls.
//!
//! The original redraws once per cell a unit moves (8 cs walking, 12 cs
//! flying, the camera re-centred each time) and once per scroll step, so
//! the map jumps 32 px at a time. With smooth motion the frames in between
//! are drawn as well, the unit and the camera at pixel positions between
//! the two cells, paced to the display's refresh. Every step still ends on
//! the original's own frame at the original's time, and the in-between
//! frames leave no trace in the game state (positions, timers, render
//! flags, the RNG): they only draw.

use s2data::sprite::Image;

use crate::game::Game;
use crate::gfx;
use crate::sys::{SCREEN_H, SCREEN_W};

use super::super::UnitRef;

/// A unit drawn away from its cell during an in-between frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nudge {
    pub unit: UnitRef,
    /// The cell it is drawn for (scenery in front, elevation).
    pub cell: (i32, i32),
    /// Pixels from where it would stand on that cell.
    pub shift: (i32, i32),
}

/// The nudge for unit `r`, if it is the one being moved.
pub fn nudge_of(game: &Game, r: UnitRef) -> Option<Nudge> {
    game.battle.view.nudge.filter(|n| n.unit == r)
}

/// Margin of the canvas the in-between frames are drawn on, in scroll
/// units (half cells of 32 px, rows of 16 px): the camera may lag this far
/// behind the step's own view.
const MARGIN_X: i32 = 2;
const MARGIN_Y: i32 = 4;

/// A step of motion: where the camera and the moving unit were before it.
#[derive(Debug, Clone, Copy)]
pub struct From {
    /// `Map::scroll_x/scroll_y` before the step.
    pub scroll: (i32, i32),
    /// The unit moving and the cell it left, if any.
    pub unit: Option<(UnitRef, (i32, i32))>,
}

impl From {
    /// The camera (and `unit`'s cell) as they are now.
    pub fn now(game: &Game, unit: Option<UnitRef>) -> From {
        let m = game.battle.map();
        From {
            scroll: (m.scroll_x, m.scroll_y),
            unit: unit.map(|r| {
                let u = game.battle.unit(r);
                (r, (u.x, u.y))
            }),
        }
    }
}

/// World position (map-surface pixels) a unit standing on (x, y) is drawn
/// relative to: its cell's corner raised by the elevation.
fn anchor(game: &Game, x: i32, y: i32) -> (i32, i32) {
    let h = game.battle.map().height(x, y);
    (x * 64 + if y & 1 != 0 { 32 } else { 0 }, y * 16 - (h - 1) * 16)
}

fn lerp(a: i32, b: i32, t: f32) -> i32 {
    a + ((b - a) as f32 * t).round() as i32
}

/// When a step that waits `n` cs after `t0` (`Game::wait_until`) began,
/// in ms: at `t0`, or now if that wait would not wait at all.
pub fn step_start(game: &Game, t0: i32, n: i32) -> u32 {
    let now = game.sys.ticks();
    if n <= 0 || (t0 - game.ticks()).abs() >= n {
        now
    } else {
        t0.max(0) as u32 * 10
    }
}

/// Present the in-between frames of a step that started at `start_ms`
/// (`Sys::ticks`) and lasts `dur_ms`. Call it once the step's own frame is
/// drawn in the current page, just before that frame's wait and present;
/// `pre` is the state taken before that frame was drawn. `draw` draws the
/// scene into the current page the way the step's frame was drawn; for each
/// in-between frame the camera and the unit are moved and the state is put
/// back to `pre` first. The page and the state are left as they were.
/// Does nothing unless smooth motion is on.
pub fn tween(game: &mut Game, from: From, pre: &Snapshot, start_ms: u32, dur_ms: u32, draw: &mut dyn FnMut(&mut Game)) {
    if !game.sys.smooth() || !super::super::actions::presenting(game) {
        return;
    }
    let to_scroll = {
        let m = game.battle.map();
        (m.scroll_x, m.scroll_y)
    };
    let moved = from.unit.and_then(|(r, cell)| {
        let u = game.battle.unit(r);
        ((u.x, u.y) != cell).then_some((r, cell, (u.x, u.y)))
    });
    if to_scroll == from.scroll && moved.is_none() {
        return;
    }
    let frame_ms = game.sys.smooth_frame_ms();
    let n = (dur_ms + frame_ms / 2) / frame_ms;
    if n < 2 {
        return;
    }
    let post = Snapshot::take(game);
    let page = game.scr.cur;
    let frame = game.scr.pages[page].clone();
    for k in 1..n {
        let at = start_ms + dur_ms * k / n;
        // Running late: skip to the next one (headless runs draw them all).
        let virtual_clock = game.sys.is_virtual();
        if !virtual_clock && game.sys.ticks() >= at {
            continue;
        }
        let t = k as f32 / n as f32;
        // Camera in surface pixels, kept within the canvas margin.
        let cam = (
            lerp(from.scroll.0 * 32, to_scroll.0 * 32, t).clamp((to_scroll.0 - MARGIN_X) * 32, (to_scroll.0 + MARGIN_X) * 32),
            lerp(from.scroll.1 * 16, to_scroll.1 * 16, t).clamp((to_scroll.1 - MARGIN_Y) * 16, (to_scroll.1 + MARGIN_Y) * 16),
        );
        game.battle.view.nudge = moved.map(|(r, a, b)| {
            let (pa, pb) = (anchor(game, a.0, a.1), anchor(game, b.0, b.1));
            let cell = if t < 0.5 { a } else { b };
            let pc = if t < 0.5 { pa } else { pb };
            Nudge { unit: r, cell, shift: (lerp(pa.0, pb.0, t) - pc.0, lerp(pa.1, pb.1, t) - pc.1) }
        });
        pre.restore(game);
        draw_panned(game, cam, draw);
        game.battle.view.nudge = None;
        let now = game.sys.ticks();
        if at > now && !virtual_clock {
            game.sys.wait_since(now, at - now);
        }
        gfx::present_paced(game, frame_ms);
    }
    post.restore(game);
    game.scr.pages[page] = frame;
}

/// Draw with the camera at `cam` (surface pixels): the scene is drawn on a
/// canvas larger than the screen with the view moved back by the margin,
/// and the screen's window of it copied to the current page.
fn draw_panned(game: &mut Game, cam: (i32, i32), draw: &mut dyn FnMut(&mut Game)) {
    let (sx, sy) = {
        let m = game.battle.map();
        (m.scroll_x, m.scroll_y)
    };
    let (ox, oy) = (sx - MARGIN_X, sy - MARGIN_Y);
    {
        let m = game.battle.map_mut();
        m.scroll_x = ox;
        m.scroll_y = oy;
    }
    let page = game.scr.cur;
    let canvas = Image::new(SCREEN_W + 2 * MARGIN_X as usize * 32, SCREEN_H + 2 * MARGIN_Y as usize * 16);
    let screen = std::mem::replace(&mut game.scr.pages[page], canvas);
    draw(game);
    let canvas = std::mem::replace(&mut game.scr.pages[page], screen);
    {
        let m = game.battle.map_mut();
        m.scroll_x = sx;
        m.scroll_y = sy;
    }
    // Restore the view's copy of the camera too (the next draw syncs it anyway).
    if let Some(v) = game.battle.view.map.as_mut() {
        v.scroll_x = sx;
        v.scroll_y = sy;
    }
    let (cx, cy) = ((cam.0 - ox * 32) as usize, (cam.1 - oy * 16) as usize);
    let dst = &mut game.scr.pages[page];
    for y in 0..SCREEN_H {
        let s = (cy + y) * canvas.width + cx;
        dst.pixels[y * SCREEN_W..(y + 1) * SCREEN_W].copy_from_slice(&canvas.pixels[s..s + SCREEN_W]);
    }
}

/// What drawing may change and an in-between frame must not: the units'
/// render flags (the hit-shake phase) and animation timers, and the
/// flyers' mount frame.
#[derive(Clone)]
pub struct Snapshot {
    units: [Vec<(u32, i32)>; 2],
    mount_phase: usize,
}

impl Snapshot {
    pub fn take(game: &Game) -> Snapshot {
        let b = &game.battle;
        let army = |s: usize| b.armies[s].units.iter().map(|u| (u.render_flags, u.anim_timer)).collect();
        Snapshot { units: [army(0), army(1)], mount_phase: b.view.mount_phase }
    }

    fn restore(&self, game: &mut Game) {
        let b = &mut game.battle;
        for (army, saved) in b.armies.iter_mut().zip(&self.units) {
            for (u, &(f, t)) in army.units.iter_mut().zip(saved) {
                u.render_flags = f;
                u.anim_timer = t;
            }
        }
        b.view.mount_phase = self.mount_phase;
    }
}
