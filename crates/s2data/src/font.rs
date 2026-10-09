//! The game's bitmap fonts and text rasterizer.
//!
//! Fonts live in SUPERSP.BIN (1-based entry ids):
//!
//! | id | contents |
//! |----|----------|
//! | 11 | glyph index: `u16 glyph_no[]`, addressed by `big5_slot(code) - 0x8000` |
//! | 12 | 16x16 1bpp glyphs, 32 bytes each (glyph_no is 1-based) |
//! | 13 | 24x24 1bpp glyphs, 72 bytes each |
//! | 14 | alternate 16x16 set (only used when the unused `DAT_004944f4` flag is set) |
//! | 15 | 256 ASCII glyphs, 8x15 1bpp, 15 bytes each |
//! | 16 | 256 ASCII glyphs, 16x24 1bpp, 48 bytes each |
//!
//! The shipped game always runs with `DAT_004944f4 == 0`, so regular text
//! (`FUN_0041f748`) uses the 24-pixel Big5 glyphs plus 16x24 ASCII, and the
//! compact text routine (`FUN_0041df60`) uses 16x16 Big5 plus 8x15 ASCII.
//!
//! Each glyph is expanded into a small cell buffer (the global at 0x492dac)
//! and then blitted with index 0 transparent. The cell geometry and the
//! order of the outline passes are reproduced exactly so that outlines come
//! out pixel-identical to the original.

use crate::archive::{Archives, Kind};
use crate::sprite::Image;
use crate::text::{chars, Ch};
use crate::{format_err, Result};

pub const BIN_INDEX: usize = 11;
pub const BIN_WIDE16: usize = 12;
pub const BIN_WIDE24: usize = 13;
pub const BIN_WIDE16_ALT: usize = 14;
pub const BIN_ASCII8: usize = 15;
pub const BIN_ASCII16: usize = 16;

/// Big5 code -> glyph index slot, exactly as `FUN_0041ecd6`.
///
/// Returns a value >= 0x8000; the glyph number is then read from the index
/// table at byte offset `slot * 2 - 0x10000`. `None` stands for the game's
/// "0" result (unmapped code).
pub fn big5_slot(hi: u8, lo: u8) -> Option<u32> {
    let (hi, lo) = (hi as u32, lo as u32);
    if hi == 0xc6 && lo > 0xa0 {
        return Some(lo + 0x7f60);
    }
    let (base, row) = match hi {
        0xa1..=0xa3 => (0x8400, hi - 0xa1),
        0xa4..=0xc6 => (0x8800, hi - 0xa4),
        0xc7..=0xc8 => (0x805f, hi - 0xc7),
        0xc9..=0xf9 => (0x9d19, hi - 0xc9),
        _ => return None,
    };
    let col = match lo {
        0x40..=0x7e => lo - 0x40,
        0xa1..=0xfe => lo - 0x62,
        _ => return None,
    };
    Some(row * 0x9d + col + base)
}

/// How a glyph is colored (`font+0x10` in the original).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// 0 (and any other value): main color with a 4-neighbour outline in the second color.
    Outline,
    /// 1: second color offset by (+1,+1), then the main color on top.
    Shadow,
    /// 2: main color only.
    Plain,
}

impl Mode {
    pub fn from_raw(v: u32) -> Mode {
        match v {
            1 => Mode::Shadow,
            2 => Mode::Plain,
            _ => Mode::Outline,
        }
    }
}

/// Colors and mode for a text draw call (`font+4`, `font+8`, `font+0x10`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub color: u8,
    pub second: u8,
    pub mode: Mode,
}

impl Style {
    pub const fn new(color: u8, second: u8, mode: Mode) -> Style {
        Style { color, second, mode }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphKind {
    Wide16,
    Wide24,
    Ascii8,
    Ascii16,
}

/// Cell buffer geometry and outline pass ranges for one glyph kind.
struct Cell {
    w: usize,
    h: usize,
    /// Top-left of the bitmap inside the cell.
    row0: usize,
    col0: usize,
    gw: usize,
    gh: usize,
    /// Horizontal passes: first row, row count, forward/backward scan lengths.
    h_row0: usize,
    h_rows: usize,
    h_fwd: usize,
    h_back: usize,
    /// Vertical passes: columns 1..v_cols, rows 0..v_down (look below), v_up..=1 (look above).
    v_cols: usize,
    v_down: usize,
    v_up: usize,
}

impl GlyphKind {
    fn cell(self) -> Cell {
        match self {
            // FUN_0041e308
            GlyphKind::Wide16 => Cell { w: 18, h: 18, row0: 0, col0: 1, gw: 16, gh: 16, h_row0: 0, h_rows: 16, h_fwd: 16, h_back: 17, v_cols: 18, v_down: 16, v_up: 17 },
            // FUN_0041eeb6
            GlyphKind::Wide24 => Cell { w: 26, h: 26, row0: 0, col0: 1, gw: 24, gh: 24, h_row0: 0, h_rows: 24, h_fwd: 24, h_back: 25, v_cols: 26, v_down: 24, v_up: 25 },
            // FUN_0041e700
            GlyphKind::Ascii8 => Cell { w: 10, h: 18, row0: 1, col0: 1, gw: 8, gh: 15, h_row0: 1, h_rows: 16, h_fwd: 8, h_back: 9, v_cols: 9, v_down: 16, v_up: 17 },
            // FUN_0041f2ab
            GlyphKind::Ascii16 => Cell { w: 18, h: 26, row0: 1, col0: 1, gw: 16, gh: 24, h_row0: 1, h_rows: 24, h_fwd: 16, h_back: 17, v_cols: 17, v_down: 24, v_up: 25 },
        }
    }

    pub fn cell_size(self) -> (usize, usize) {
        let c = self.cell();
        (c.w, c.h)
    }
}

/// Rasterize a 1bpp bitmap into a cell image styled like the original.
/// For [`Mode::Shadow`] this returns the shadow cell and the main cell.
fn raster(kind: GlyphKind, bits: &[u8], style: Style) -> (Image, Option<Image>) {
    let c = kind.cell();
    let bpr = c.gw / 8;
    let mut buf = vec![0u8; c.w * c.h];
    let first = if style.mode == Mode::Shadow { style.second } else { style.color };
    for y in 0..c.gh {
        for x in 0..c.gw {
            let byte = bits.get(y * bpr + x / 8).copied().unwrap_or(0);
            if byte & (0x80 >> (x % 8)) != 0 {
                buf[(c.row0 + y) * c.w + c.col0 + x] = first;
            }
        }
    }
    match style.mode {
        Mode::Plain => (Image { width: c.w, height: c.h, pixels: buf }, None),
        Mode::Shadow => {
            let main = buf.iter().map(|&p| if p != 0 { style.color } else { 0 }).collect();
            (Image { width: c.w, height: c.h, pixels: buf }, Some(Image { width: c.w, height: c.h, pixels: main }))
        }
        Mode::Outline => {
            let (main, edge) = (style.color, style.second);
            for r in 0..c.h_rows {
                let s = (c.h_row0 + r) * c.w;
                for k in 0..c.h_fwd {
                    if buf[s + k] == 0 && buf[s + k + 1] == main {
                        buf[s + k] = edge;
                    }
                }
                for k in (1..=c.h_back).rev() {
                    if buf[s + k] == 0 && buf[s + k - 1] == main {
                        buf[s + k] = edge;
                    }
                }
            }
            for col in 1..c.v_cols {
                for row in 0..c.v_down {
                    if buf[row * c.w + col] == 0 && buf[(row + 1) * c.w + col] == main {
                        buf[row * c.w + col] = edge;
                    }
                }
                for row in (1..=c.v_up).rev() {
                    if buf[row * c.w + col] == 0 && buf[(row - 1) * c.w + col] == main {
                        buf[row * c.w + col] = edge;
                    }
                }
            }
            (Image { width: c.w, height: c.h, pixels: buf }, None)
        }
    }
}

/// All font data needed to draw text.
#[derive(Debug, Clone)]
pub struct Font {
    index: Vec<u8>,
    wide16: Vec<u8>,
    wide24: Vec<u8>,
    ascii8: Vec<u8>,
    ascii16: Vec<u8>,
}

impl Font {
    /// Load the fonts the shipped game uses. With `alt_wide16`, the 16x16
    /// set comes from BIN 14 instead of BIN 12 (the `DAT_004944f4` variant).
    pub fn load(arcs: &Archives, alt_wide16: bool) -> Result<Font> {
        let wide16_id = if alt_wide16 { BIN_WIDE16_ALT } else { BIN_WIDE16 };
        Font::from_parts(
            arcs.read(Kind::Bin, BIN_INDEX)?,
            arcs.read(Kind::Bin, wide16_id)?,
            arcs.read(Kind::Bin, BIN_WIDE24)?,
            arcs.read(Kind::Bin, BIN_ASCII8)?,
            arcs.read(Kind::Bin, BIN_ASCII16)?,
        )
    }

    pub fn from_parts(index: Vec<u8>, wide16: Vec<u8>, wide24: Vec<u8>, ascii8: Vec<u8>, ascii16: Vec<u8>) -> Result<Font> {
        if ascii8.len() < 256 * 15 || ascii16.len() < 256 * 48 {
            return Err(format_err("font", "ASCII glyph tables too short"));
        }
        Ok(Font { index, wide16, wide24, ascii8, ascii16 })
    }

    /// Number of glyphs in the Big5 banks.
    pub fn glyph_count(&self) -> usize {
        self.wide16.len() / 32
    }

    /// 1-based glyph number for a Big5 code, if mapped.
    pub fn glyph_number(&self, hi: u8, lo: u8) -> Option<usize> {
        let off = (big5_slot(hi, lo)? as usize * 2).checked_sub(0x10000)?;
        let n = u16::from_le_bytes([*self.index.get(off)?, *self.index.get(off + 1)?]) as usize;
        (n > 0).then_some(n)
    }

    /// Raw bitmap of a glyph by kind. For ASCII kinds `hi` is the byte and `lo` is ignored.
    pub fn bits(&self, kind: GlyphKind, hi: u8, lo: u8) -> Option<&[u8]> {
        let (bank, size) = match kind {
            GlyphKind::Ascii8 => return self.ascii8.get(hi as usize * 15..hi as usize * 15 + 15),
            GlyphKind::Ascii16 => return self.ascii16.get(hi as usize * 48..hi as usize * 48 + 48),
            GlyphKind::Wide16 => (&self.wide16, 32),
            GlyphKind::Wide24 => (&self.wide24, 72),
        };
        let n = self.glyph_number(hi, lo)?;
        bank.get((n - 1) * size..n * size)
    }

    /// Bitmap of glyph number `n` (1-based) from a Big5 bank, for font sheets.
    pub fn bits_by_number(&self, kind: GlyphKind, n: usize) -> Option<&[u8]> {
        let (bank, size) = match kind {
            GlyphKind::Wide16 => (&self.wide16, 32),
            GlyphKind::Wide24 => (&self.wide24, 72),
            _ => return None,
        };
        bank.get(n.checked_sub(1)? * size..n * size)
    }

    /// Draw one glyph cell with its top-left at (x, y).
    pub fn draw_glyph(&self, img: &mut Image, kind: GlyphKind, hi: u8, lo: u8, x: isize, y: isize, style: Style) {
        let Some(bits) = self.bits(kind, hi, lo) else { return };
        draw_bits(img, kind, bits, x, y, style);
    }

    /// Single-line text in the regular (large) font, as `FUN_0041f748`:
    /// ASCII is drawn at x+2 and advances 16, Big5 advances 24. Bytes below
    /// 0x1e are drawn as glyph 0x1e. Returns the x after the last character.
    pub fn draw_line(&self, img: &mut Image, mut x: isize, y: isize, text: &[u8], style: Style) -> isize {
        for ch in chars(text) {
            match ch {
                Ch::Byte(c) => {
                    self.draw_glyph(img, GlyphKind::Ascii16, c.max(0x1e), 0, x + 2, y, style);
                    x += 16;
                }
                Ch::Wide(hi, lo) => {
                    self.draw_glyph(img, GlyphKind::Wide24, hi, lo, x, y, style);
                    x += 24;
                }
            }
        }
        x
    }

    /// Single-line text in the compact font, as `FUN_0041df60`: ASCII
    /// advances 8, Big5 advances 16.
    pub fn draw_line_small(&self, img: &mut Image, mut x: isize, y: isize, text: &[u8], style: Style) -> isize {
        for ch in chars(text) {
            match ch {
                Ch::Byte(c) => {
                    self.draw_glyph(img, GlyphKind::Ascii8, c.max(0x1e), 0, x, y, style);
                    x += 8;
                }
                Ch::Wide(hi, lo) => {
                    self.draw_glyph(img, GlyphKind::Wide16, hi, lo, x, y, style);
                    x += 16;
                }
            }
        }
        x
    }

    /// Boxed multi-line text, as `FUN_0041f87c` in the shipped (large) mode.
    ///
    /// Each line holds up to `cols * 2 - 1` bytes; every character occupies a
    /// 28-pixel slot, with ASCII pulled 8 pixels left per preceding ASCII
    /// character. '*' ends a line (ignored at a line start). After `lines`
    /// lines the original wraps back to the top row and keeps drawing; that
    /// quirk is kept.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_wrapped(&self, img: &mut Image, x: isize, y: isize, text: &[u8], style: Style, lines: usize, cols: usize, line_h: isize) {
        let mut p = 0usize;
        let budget = cols * 2;
        if lines == 0 || budget <= 1 {
            return;
        }
        loop {
            for line in 0..lines {
                let mut slot: isize = 0;
                let mut ascii_shift: isize = 0;
                let mut used = 1usize;
                let ly = y + line as isize * line_h;
                while used < budget {
                    let c = text.get(p).copied().unwrap_or(0);
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
                            self.draw_glyph(img, GlyphKind::Ascii16, c.max(0x1e), 0, x + slot * 28 - ascii_shift, ly, style);
                            ascii_shift += 8;
                            used += 1;
                        }
                    } else {
                        let lo = text.get(p + 1).copied().unwrap_or(0);
                        self.draw_glyph(img, GlyphKind::Wide24, c, lo, x + slot * 28 - ascii_shift, ly, style);
                        used += 2;
                        p += 2;
                    }
                    slot += 1;
                }
            }
        }
    }

    /// Pixel width `draw_line` would advance for `text`.
    pub fn line_width(text: &[u8]) -> isize {
        chars(text).iter().map(|c| if matches!(c, Ch::Byte(_)) { 16 } else { 24 }).sum()
    }
}

/// Rasterize and blit a raw glyph bitmap.
pub fn draw_bits(img: &mut Image, kind: GlyphKind, bits: &[u8], x: isize, y: isize, style: Style) {
    let (first, second) = raster(kind, bits, style);
    match second {
        Some(main) => {
            img.blit(&first, x + 1, y + 1);
            img.blit(&main, x, y);
        }
        None => img.blit(&first, x, y),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_matches_game_mapping() {
        // First code of the frequent-character block.
        assert_eq!(big5_slot(0xa4, 0x40), Some(0x8800));
        assert_eq!(big5_slot(0xa4, 0xa1), Some(0x8800 + 0x3f));
        assert_eq!(big5_slot(0xa5, 0x40), Some(0x8800 + 0x9d));
        assert_eq!(big5_slot(0xa1, 0x40), Some(0x8400));
        assert_eq!(big5_slot(0xc9, 0x40), Some(0x9d19));
        assert_eq!(big5_slot(0xc6, 0xa1), Some(0xa1 + 0x7f60));
        assert_eq!(big5_slot(0xfa, 0x40), None);
        assert_eq!(big5_slot(0xa4, 0x80), None);
    }

    #[test]
    fn outline_surrounds_a_dot() {
        // A single pixel at glyph (0,1) of an 8x15 ASCII glyph (cell row 2, col 1).
        let mut bits = [0u8; 15];
        bits[1] = 0x80;
        let (img, none) = raster(GlyphKind::Ascii8, &bits, Style::new(5, 9, Mode::Outline));
        assert!(none.is_none());
        let at = |x: usize, y: usize| img.pixels[y * img.width + x];
        assert_eq!(at(1, 2), 5);
        assert_eq!((at(0, 2), at(2, 2), at(1, 1), at(1, 3)), (9, 9, 9, 9));
        assert_eq!(at(0, 1), 0, "diagonals are not outlined");
    }

    #[test]
    fn shadow_returns_two_layers() {
        let mut bits = [0u8; 32];
        bits[0] = 0x80;
        let (shadow, main) = raster(GlyphKind::Wide16, &bits, Style::new(7, 3, Mode::Shadow));
        let main = main.unwrap();
        assert_eq!(shadow.pixels[1], 3);
        assert_eq!(main.pixels[1], 7);
    }
}
