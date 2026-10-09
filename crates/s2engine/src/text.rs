//! The text renderer object `DAT_00492d38` (docs/ui.md §7). Glyph
//! rasterizing is `s2data::font`; this keeps the object's state and the
//! call conventions of the original entry points.

use s2data::font::{GlyphKind, Mode, Style};
use s2data::sprite::Image;

use crate::game::Game;

/// `+0x04 fg, +0x08 second colour, +0x0c target (0 = visible screen, 1 = current page), +0x10 style`.
#[derive(Debug, Clone, Copy)]
pub struct TextState {
    pub fg: u8,
    pub c2: u8,
    pub target: u32,
    pub style: u32,
}

impl Default for TextState {
    fn default() -> Self {
        TextState { fg: 0x2a, c2: 0, target: 1, style: 2 }
    }
}

impl TextState {
    fn style(&self) -> Style {
        Style::new(self.fg, self.c2, Mode::from_raw(self.style))
    }
}

/// Run `f` on the draw target selected by `game.text.target`. Target 0 is
/// the visible screen (`FUN_0046e5c1` on `DAT_004a2b00`), shown at once.
fn with_target(game: &mut Game, f: impl FnOnce(&s2data::font::Font, &mut Image)) {
    let font = &game.res.as_ref().expect("resources").font;
    if game.text.target == 0 {
        f(font, &mut game.sys.fb);
        game.sys.flip();
    } else {
        let cur = game.scr.cur;
        f(font, &mut game.scr.pages[cur]);
    }
}

/// Shared body of `FUN_0041df60` / `FUN_0041f748`: set +4..+0x10, draw,
/// restore +4/+8/+0xc (the style at +0x10 stays as set, like the original).
#[allow(clippy::too_many_arguments)]
fn draw(game: &mut Game, large: bool, x: i32, y: i32, s: &[u8], fg: u8, c2: u8, target: u32, style: u32) {
    let saved = game.text;
    game.text = TextState { fg, c2, target, style };
    let st = game.text.style();
    let mainland = game.cfg.mainland;
    with_target(game, |font, img| {
        if large && mainland {
            draw_line_mainland(font, img, x, y, s, st);
        } else if large {
            font.draw_line(img, x as isize, y as isize, s, st);
        } else {
            font.draw_line_small(img, x as isize, y as isize, s, st);
        }
    });
    game.text = TextState { style, ..saved };
}

/// `FUN_0041ec63` / `FUN_0041ec91` pick a glyph (`FUN_0041ee1a` 16-pixel bank,
/// `FUN_0041ee68` 24-pixel bank; s2data `Font::draw_glyph`).
/// `FUN_0041f748` with `DAT_004944f4` (mainland) set: Big5 characters come
/// from the 16-pixel bank (`FUN_0041ec91` → `FUN_0041ee1a`) drawn at
/// (x+4, y+4) in the same 24-pixel cell (`FUN_0041e308`).
fn draw_line_mainland(font: &s2data::font::Font, img: &mut Image, mut x: i32, y: i32, s: &[u8], st: Style) {
    let mut p = 0;
    while let Some(&c) = s.get(p) {
        if c == 0 || c == b'\n' {
            break;
        }
        if c < 0x81 {
            font.draw_glyph(img, GlyphKind::Ascii16, c.max(0x1e), 0, (x + 2) as isize, y as isize, st);
            x += 0x10;
            p += 1;
        } else {
            let lo = s.get(p + 1).copied().unwrap_or(0);
            font.draw_glyph(img, GlyphKind::Wide16, c, lo, (x + 4) as isize, (y + 4) as isize, st);
            x += 0x18;
            p += 2;
        }
    }
}

/// `FUN_0041df60`: compact font (ASCII 8 px, Big5 16 px).
#[allow(clippy::too_many_arguments)]
pub fn small(game: &mut Game, x: i32, y: i32, s: &[u8], fg: u8, c2: u8, target: u32, style: u32) {
    draw(game, false, x, y, s, fg, c2, target, style);
}

/// `FUN_0041f748`: regular font (ASCII 16 px drawn at x+2, Big5 24 px).
#[allow(clippy::too_many_arguments)]
pub fn large(game: &mut Game, x: i32, y: i32, s: &[u8], fg: u8, c2: u8, target: u32, style: u32) {
    draw(game, true, x, y, s, fg, c2, target, style);
}

/// `FUN_0041deeb`: a number with format `fmt` ∈ {"%d","%1d",...,"%5d"} in the compact font.
#[allow(clippy::too_many_arguments)]
pub fn number(game: &mut Game, x: i32, y: i32, value: i32, fg: u8, c2: u8, target: u32, style: u32, fmt: usize) {
    let s = if fmt == 0 { format!("{value}") } else { format!("{value:>w$}", w = fmt) };
    small(game, x, y, s.as_bytes(), fg, c2, target, style);
}

/// `FUN_0041f87c`: wrapped regular-font text (`*` breaks lines).
#[allow(clippy::too_many_arguments)]
/// With `DAT_004944f4` (mainland) set, characters sit in 0x14-pixel slots and
/// Big5 ones are 16-pixel glyphs drawn 4 pixels lower.
pub fn block(game: &mut Game, x: i32, y: i32, s: &[u8], fg: u8, c2: u8, target: u32, style: u32, rows: usize, cols: usize, line_h: i32) {
    game.text = TextState { fg, c2, target, style };
    let st = game.text.style();
    if !game.cfg.mainland {
        with_target(game, |font, img| font.draw_wrapped(img, x as isize, y as isize, s, st, rows, cols, line_h as isize));
        return;
    }
    let budget = cols * 2;
    if rows == 0 || budget <= 1 {
        return;
    }
    with_target(game, |font, img| {
        let mut p = 0usize;
        loop {
            for row in 0..rows {
                let (mut slot, mut shift, mut used) = (0i32, 0i32, 1usize);
                let ly = y + row as i32 * line_h;
                while used < budget {
                    let c = s.get(p).copied().unwrap_or(0);
                    if c == 0 || c == b'\n' {
                        return;
                    }
                    if c < 0x81 {
                        p += 1;
                        if c == b'*' {
                            if slot == 0 {
                                slot = -1;
                            } else {
                                used = budget;
                            }
                        } else {
                            font.draw_glyph(img, GlyphKind::Ascii16, c.max(0x1e), 0, (x + slot * 0x14 - shift) as isize, ly as isize, st);
                            shift += 8;
                            used += 1;
                        }
                    } else {
                        let lo = s.get(p + 1).copied().unwrap_or(0);
                        font.draw_glyph(img, GlyphKind::Wide16, c, lo, (x + slot * 0x14 - shift) as isize, (ly + 4) as isize, st);
                        used += 2;
                        p += 2;
                    }
                    slot += 1;
                }
            }
        }
    });
}

/// `FUN_0041e2e0`: set fg, second colour and target.
pub fn set(game: &mut Game, fg: u8, c2: u8, target: u32) {
    game.text.fg = fg;
    game.text.c2 = c2;
    game.text.target = target;
}

/// Pen of the typewriters: `pen.0` is the x coordinate (`param_1[0]`).
/// `count` is `*param_4`: bytes put on the current line, or -1 after `*`.
///
/// Port of `FUN_0041fa6c(this, pen, y, text, count, style)`: draw one
/// character of `text` in the regular font with the current colours and
/// return the rest of the text. NUL and `\n` are not consumed.
pub fn put_char<'t>(game: &mut Game, pen: &mut i32, y: i32, text: &'t [u8], count: &mut i32, style: u32) -> &'t [u8] {
    game.text.style = style;
    let Some(&c) = text.first() else { return text };
    if c == b'\n' || c == 0 {
        return text;
    }
    let st = game.text.style();
    if c < 0x81 {
        if c == b'*' {
            *count = -1;
        } else {
            let x = *pen;
            with_target(game, |font, img| font.draw_glyph(img, GlyphKind::Ascii16, c.max(0x1e), 0, x as isize, y as isize, st));
            // Mainland mode (`DAT_004944f4`) packs ASCII 8 pixels apart.
            *pen += if game.cfg.mainland { 8 } else { 0x10 };
            *count += 1;
        }
        &text[1..]
    } else {
        let lo = text.get(1).copied().unwrap_or(0);
        let x = *pen;
        if game.cfg.mainland {
            with_target(game, |font, img| font.draw_glyph(img, GlyphKind::Wide16, c, lo, x as isize, (y + 4) as isize, st));
            *pen += 0x10;
        } else {
            with_target(game, |font, img| font.draw_glyph(img, GlyphKind::Wide24, c, lo, x as isize, y as isize, st));
            *pen += 0x18;
        }
        *count += 2;
        &text[2.min(text.len())..]
    }
}

/// Byte length the way `strlen` counts it (up to NUL), for the centring idioms.
pub fn strlen(s: &[u8]) -> i32 {
    s.iter().position(|&c| c == 0).unwrap_or(s.len()) as i32
}

/// `FUN_0041e06d`: wrapped compact-font text. Characters sit in 16-pixel
/// slots (ASCII pulled 8 px left per preceding ASCII character), up to
/// `cols * 2 - 1` bytes per row, `*` breaks a row, after `rows` rows it
/// starts over at the top like the original.
#[allow(clippy::too_many_arguments)]
pub fn block_small(game: &mut Game, x: i32, y: i32, s: &[u8], fg: u8, c2: u8, target: u32, style: u32, rows: usize, cols: usize, line_h: i32) {
    game.text = TextState { fg, c2, target, style };
    let st = game.text.style();
    let budget = cols * 2;
    if rows == 0 || budget <= 1 {
        return;
    }
    with_target(game, |font, img| {
        let mut p = 0usize;
        loop {
            for row in 0..rows {
                let mut slot: i32 = 0;
                let mut shift: i32 = 0;
                let mut used = 1usize;
                let ly = y + row as i32 * line_h;
                while used < budget {
                    let c = s.get(p).copied().unwrap_or(0);
                    if c == 0 || c == b'\n' {
                        return;
                    }
                    if c < 0x81 {
                        p += 1;
                        if c == b'*' {
                            if slot == 0 {
                                slot = -1;
                            } else {
                                used = budget;
                            }
                        } else {
                            font.draw_glyph(img, GlyphKind::Ascii8, c.max(0x1e), 0, (x + slot * 0x10 - shift) as isize, ly as isize, st);
                            shift += 8;
                            used += 1;
                        }
                    } else {
                        let lo = s.get(p + 1).copied().unwrap_or(0);
                        font.draw_glyph(img, GlyphKind::Wide16, c, lo, (x + slot * 0x10 - shift) as isize, ly as isize, st);
                        used += 2;
                        p += 2;
                    }
                    slot += 1;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mainland_mode_packs_the_typewriter() {
        let dir = std::env::var_os("S2_GAME_DIR")
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("games/super2sp/extracted/app")));
        let Some(data) = dir.and_then(|d| crate::Data::open(&d).ok()) else { return eprintln!("text test: no game data, skipped") };
        let mut sys = crate::sys::Sys::new_headless(crate::sys::Shared::new(), Box::new(|_, _| {}));
        let mut game = Game::new(&mut sys, &data);
        game.res = Some(crate::res::Res::load(&data, &game.cfg).unwrap());
        let text = "A\u{0}".as_bytes();
        let big5 = [0xa4u8, 0x40, 0];
        for (mainland, ascii, wide) in [(false, 0x10, 0x18), (true, 8, 0x10)] {
            game.cfg.mainland = mainland;
            let (mut pen, mut count) = (0, 0);
            put_char(&mut game, &mut pen, 0, text, &mut count, 2);
            assert_eq!(pen, ascii, "FUN_0041fa6c ASCII advance, mainland={mainland}");
            let mut pen = 0;
            put_char(&mut game, &mut pen, 0, &big5, &mut count, 2);
            assert_eq!(pen, wide, "FUN_0041fa6c Big5 advance, mainland={mainland}");
        }
    }
}
