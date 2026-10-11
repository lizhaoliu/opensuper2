//! The screen object `DAT_004947c8` (three 640x480 pages), its drawing
//! primitives, presenting, and the palette operations (docs/ui.md §1–2).
//!
//! All drawing goes to the current page. Presenting copies a page to
//! [`crate::sys::Sys::fb`] (the original's DirectDraw surface) and flips.

use s2data::gfx_palette::vga_to_rgb;
use s2data::sprite::Image;
use s2data::Palette;

use crate::game::Game;
use crate::sys::{SCREEN_H, SCREEN_W};

pub const PAGES: usize = 3;

/// `DAT_004947c8` plus the hardware palette kept by the DirectDraw wrapper.
pub struct Screen {
    pub pages: Vec<Image>,
    /// `+0x08` current page.
    pub cur: usize,
    /// The palette in effect (8-bit RGB), what fades and cycling modify.
    pub hw: [[u8; 3]; 256],
    /// Last presented page content, re-presented when the palette changes.
    shown: Image,
    /// `DAT_0048f128`: palette cycling ranges `{start, count, period_cs, last}`.
    cycle: [[i32; 4]; 3],
}

impl Default for Screen {
    fn default() -> Self {
        Screen::new()
    }
}

impl Screen {
    pub fn new() -> Screen {
        Screen {
            pages: (0..PAGES).map(|_| Image::new(SCREEN_W, SCREEN_H)).collect(),
            cur: 0,
            hw: [[0; 3]; 256],
            shown: Image::new(SCREEN_W, SCREEN_H),
            cycle: [[3, 8, 16, 0], [11, 8, 9, 0], [19, 13, 9, 0]],
        }
    }

    /// The current page.
    pub fn page(&mut self) -> &mut Image {
        &mut self.pages[self.cur]
    }

    /// `FUN_0046eb02`: select the current page (ignored when out of range).
    pub fn set_page(&mut self, p: usize) {
        if p < PAGES {
            self.cur = p;
        }
    }

    /// Copy page `src` to page `dst`: `FUN_0046ec6b(scr, src, dst)` (note the order).
    pub fn copy_page(&mut self, dst: usize, src: usize) {
        if dst != src {
            let s = self.pages[src].pixels.clone();
            self.pages[dst].pixels.copy_from_slice(&s);
        }
    }

    /// `FUN_0046ecc2`: clear the current page.
    pub fn clear(&mut self) {
        self.page().pixels.fill(0);
    }

    /// `FUN_0046ece7(scr, p)`.
    pub fn clear_page(&mut self, p: usize) {
        self.pages[p].pixels.fill(0);
    }
}

/// `FUN_0046eb48`: present the current page.
pub fn present(game: &mut Game) {
    let p = game.scr.cur;
    present_page(game, p);
}

/// `S2_PRESENT_TRACE=1`: log each present with the engine functions on the
/// stack (oracle comparisons against `--watch 46eb48`).
fn present_trace(game: &Game, what: &str) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ON.get_or_init(|| std::env::var_os("S2_PRESENT_TRACE").is_some()) {
        return;
    }
    let bt = std::backtrace::Backtrace::force_capture().to_string();
    let frames: Vec<&str> = bt
        .lines()
        .filter_map(|l| l.trim().split_once(": ").map(|(_, f)| f))
        .filter(|f| f.starts_with("s2engine::") && !f.contains("gfx::present"))
        .take(4)
        .collect();
    eprintln!("[present] t{} {what} <- {}", game.ticks(), frames.join(" <- "));
}

/// `FUN_0046eb8a`: present page `p` without changing the current page.
pub fn present_page(game: &mut Game, p: usize) {
    present_trace(game, "page");
    let scr = &mut game.scr;
    scr.shown.pixels.copy_from_slice(&scr.pages[p].pixels);
    game.sys.fb.pixels.copy_from_slice(&scr.shown.pixels);
    game.sys.palette = Palette(scr.hw);
    game.sys.flip();
}

/// Present the current page paced to `ms` (smooth-motion frames, which
/// are not the original's and are presented at the display's rate).
pub fn present_paced(game: &mut Game, ms: u32) {
    if game.sys.is_virtual() {
        // Headless: the frame only goes to `S2_TWEEN_DIR`; what is shown stays.
        let page = &game.scr.pages[game.scr.cur].pixels;
        game.sys.tween_shot(page, &Palette(game.scr.hw));
        return;
    }
    let scr = &mut game.scr;
    scr.shown.pixels.copy_from_slice(&scr.pages[scr.cur].pixels);
    game.sys.fb.pixels.copy_from_slice(&scr.shown.pixels);
    game.sys.palette = Palette(scr.hw);
    game.sys.flip_paced(ms);
}

/// Re-present the last frame after a palette change (`FUN_00473144`).
fn present_palette(game: &mut Game) {
    present_trace(game, "palette");
    game.sys.fb.pixels.copy_from_slice(&game.scr.shown.pixels);
    game.sys.palette = Palette(game.scr.hw);
    game.sys.flip();
}

/// `FUN_0046d390`: set all 256 entries from a 6-bit palette and re-present.
pub fn set_palette(game: &mut Game, pal6: &[u8]) {
    game.scr.hw = vga_to_rgb(pal6);
    present_palette(game);
}

/// `FUN_0046d442(pal, dir, speed)`: dir 0 fades in from black to `pal6`,
/// dir 1 fades the current palette out to black. `speed < 0` steps by
/// `|speed|` levels (64 levels in all). Each step presents twice: the
/// palette scale `FUN_00474fc3` re-presents and then drains every pending
/// event (`FUN_00474c54(0xffff)`, so input during a fade is lost), and
/// `FUN_00473144` presents again.
#[allow(clippy::needless_range_loop)] // index math as in the original's loop
pub fn fade(game: &mut Game, pal6: &[u8], dir: i32, speed: i32) {
    let step = if speed < 0 { speed.unsigned_abs() as usize } else { 1 };
    let src: [[u8; 3]; 256] = if dir == 0 { vga_to_rgb(pal6) } else { game.scr.hw };
    let levels: Vec<u32> = if dir == 0 { (0..0x40).step_by(step).collect() } else { (0..0x40).rev().step_by(step).collect() };
    let pace = std::mem::replace(&mut game.sys.pace_ms, game.sys.fade_pace_ms);
    for l in levels {
        let level = l << 2;
        // FUN_004758c3 on entries 1..=255; entry 0 keeps its value.
        for (i, c) in src.iter().enumerate().skip(1) {
            for k in 0..3 {
                game.scr.hw[i][k] = (c[k] as u32 * level / 0xff) as u8;
            }
        }
        present_palette(game);
        game.flush_events(0xffff);
        present_palette(game);
    }
    game.sys.pace_ms = pace;
}

/// `FUN_0046d2da`: rotate the three cycling ranges whose period elapsed.
pub fn palette_cycle(game: &mut Game) {
    let now = game.ticks();
    let mut changed = false;
    for i in 0..3 {
        let [start, count, period, last] = game.scr.cycle[i];
        if (now - last).abs() > period {
            changed = true;
            // FUN_00475780 dir 1: the last entry moves to the front.
            let (s, n) = (start as usize, count as usize);
            if std::env::var_os("S2_PRESENT_TRACE").is_some() {
                eprintln!("[cycle] t{now} range {i}");
            }
            game.scr.hw[s..s + n].rotate_right(1);
            game.scr.cycle[i][3] = now;
        }
    }
    if changed {
        present_palette(game);
    }
}

/// `FUN_0046f629` / `FUN_0046f02f` with a decoded image: `opaque` also
/// writes index 0. (RLE literal runs containing 0 would be drawn even when
/// transparent in the original; the shipped images have none.)
pub fn blit(page: &mut Image, img: &Image, x: i32, y: i32, opaque: bool) {
    if opaque {
        page.blit_opaque(img, x as isize, y as isize);
    } else {
        page.blit(img, x as isize, y as isize);
    }
}

/// `FUN_0046fad2`.
pub fn put_pixel(page: &mut Image, x: i32, y: i32, c: u8) {
    if (0..SCREEN_W as i32).contains(&x) && (0..SCREEN_H as i32).contains(&y) {
        page.pixels[y as usize * SCREEN_W + x as usize] = c;
    }
}

/// `FUN_0046fb13(scr, y, x0, x1, c)` (unclipped in the original).
pub fn hline(page: &mut Image, y: i32, x0: i32, x1: i32, c: u8) {
    for x in x0..=x1 {
        put_pixel(page, x, y, c);
    }
}

/// `FUN_0046fb6c(scr, x, y0, y1, c)`; draws at least one pixel.
pub fn vline(page: &mut Image, x: i32, y0: i32, y1: i32, c: u8) {
    for y in y0..=y1.max(y0) {
        put_pixel(page, x, y, c);
    }
}

/// `FUN_0046fbd3(scr, x0, y0, x1, y1, c, filled)`.
pub fn rect(page: &mut Image, x0: i32, y0: i32, x1: i32, y1: i32, c: u8, filled: bool) {
    if filled {
        for y in y0..=y1.max(y0) {
            hline(page, y, x0, x1, c);
        }
    } else {
        hline(page, y0, x0, x1, c);
        for y in y0 + 1..y1 {
            put_pixel(page, x0, y, c);
            put_pixel(page, x1, y, c);
        }
        hline(page, y1.max(y0 + 1), x0, x1, c);
    }
}

/// `FUN_0046f907`: grab the rectangle (inclusive corners) into an image;
/// parts outside the screen read as 0.
pub fn grab(page: &Image, x0: i32, y0: i32, x1: i32, y1: i32) -> Image {
    let (w, h) = ((x1 - x0 + 1).max(1) as usize, (y1 - y0 + 1).max(1) as usize);
    let mut out = Image::new(w, h);
    for y in 0..h as i32 {
        let sy = y0 + y;
        if !(0..SCREEN_H as i32).contains(&sy) {
            continue;
        }
        for x in 0..w as i32 {
            let sx = x0 + x;
            if (0..SCREEN_W as i32).contains(&sx) {
                out.pixels[(y * w as i32 + x) as usize] = page.pixels[sy as usize * SCREEN_W + sx as usize];
            }
        }
    }
    out
}

/// `FUN_00420710(buf, lut, n)` over a whole page.
pub fn remap_page(page: &mut Image, lut: &[u8; 256]) {
    for p in page.pixels.iter_mut() {
        *p = lut[*p as usize];
    }
}

/// `FUN_00420bf5(x, y, img, 1)`: darken an 8-pixel band left of and below
/// the rectangle of `img` drawn at (x, y) through LUT 6.
pub fn window_shadow(page: &mut Image, lut: &[u8; 256], x: i32, y: i32, img_w: i32, img_h: i32) {
    let w = img_w;
    // `img[1] - 9` where img[1] holds height-1.
    let rows = img_h - 1 - 9;
    if rows <= 0 || w <= 0 || rows >= 0x1d5 || w >= 0x277 {
        return;
    }
    let stride = SCREEN_W as i32;
    let mut remap = |i: i32| {
        if let Some(p) = page.pixels.get_mut(i as usize) {
            *p = lut[*p as usize];
        }
    };
    let mut p;
    if x < 8 {
        // No left band; the bottom band starts at column 0.
        if y + 0x12 + rows > 0x1de {
            return;
        }
        p = (y + 8 + rows) * stride;
    } else {
        p = (y + 8) * stride + x - 8;
        for _ in 0..rows {
            for k in 0..8 {
                remap(p + k);
            }
            p += stride;
        }
    }
    for _ in 0..8 {
        for k in 0..w {
            remap(p + k);
        }
        p += stride;
    }
}

/// `FUN_0040f08e(x, y, w, h)`: bevelled box.
pub fn bevel_box(page: &mut Image, x: i32, y: i32, w: i32, h: i32) {
    rect(page, x, y, x - 1 + w, y - 1 + h, 0xf0, true);
    hline(page, y + 1, x + 1, x - 2 + w, 0xfe);
    vline(page, x + 1, y + 2, y - 2 + h, 0xfe);
    hline(page, y - 1 + h, x + 1, x - 1 + w, 0xed);
    vline(page, x - 1 + w, y + 1, y - 2 + h, 0xed);
    hline(page, y + h, x, x + w, 0x7d);
    vline(page, x + w, y, y - 1 + h, 0x7d);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bump() -> [u8; 256] {
        std::array::from_fn(|i| (i as u8).wrapping_add(1))
    }

    #[test]
    fn shadow_covers_left_and_bottom_bands() {
        let mut page = Image::new(SCREEN_W, SCREEN_H);
        // A 100x50 window at (100, 100): rows = 50 - 1 - 9 = 40.
        window_shadow(&mut page, &bump(), 100, 100, 100, 50);
        let at = |x: usize, y: usize| page.pixels[y * SCREEN_W + x];
        // Left band: x 92..100, y 108..148.
        assert_eq!((at(92, 108), at(99, 147), at(100, 108), at(91, 108)), (1, 1, 0, 0));
        // Bottom band: x 92..192, y 148..156.
        assert_eq!((at(92, 148), at(191, 155), at(192, 148), at(150, 156)), (1, 1, 0, 0));
        assert_eq!(at(150, 147), 0, "the window area itself is untouched above the band");
    }

    #[test]
    fn outline_rect_has_at_least_two_rows() {
        let mut page = Image::new(SCREEN_W, SCREEN_H);
        rect(&mut page, 10, 10, 20, 10, 7, false);
        assert_eq!(page.pixels[10 * SCREEN_W + 15], 7);
        assert_eq!(page.pixels[11 * SCREEN_W + 15], 7);
    }

    #[test]
    fn grab_reads_zero_off_screen() {
        let mut page = Image::new(SCREEN_W, SCREEN_H);
        page.pixels.fill(5);
        let g = grab(&page, -2, -2, 1, 1);
        assert_eq!((g.width, g.height), (4, 4));
        assert_eq!((g.pixels[0], g.pixels[15]), (0, 5));
    }
}
