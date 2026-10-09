//! Resources the original loads at start-up (`FUN_00424450`) and its
//! on-demand image loaders (docs/ui.md §6).

use anyhow::{bail, Context, Result};
use s2data::font::Font;
use s2data::gfx_palette::{tint_tables, ColorTree};
use s2data::gfx_pcx::{decode_pcx, DAT_PICTURE_BASE};
use s2data::gfx_portrait::Portraits;
use s2data::gfx_ui::{dat7_images, UiImages, DAT_UI_COUNT};
use s2data::msg::parse_indexed_strings;
use s2data::sprite::{Image, ImageBank};
use s2data::{Archives, Kind};

use crate::game::Config;
use crate::Data;

/// Resident UI images: DAT 5 sub-images 0..141 and CEL 2 sub-images 0..57.
pub const SYSIMG_RESIDENT: usize = DAT_UI_COUNT + 0x3a;

pub struct Res {
    /// `DAT_004a0b08`: master palette, 6-bit, entry 0 forced to black.
    pub master: Vec<u8>,
    /// `DAT_00494768`: UI strings (BIN 5, or BIN 6 in mainland mode).
    pub ui: Vec<Vec<u8>>,
    ui_images: UiImages,
    /// `(&DAT_00493848)[0..200]`, 0-based slots (`FUN_00421f14`).
    pub sysimg: Vec<Image>,
    pub dat7: ImageBank,
    pub font: Font,
    pub portraits: Portraits,
    /// `DAT_0049fe08`: the 13 remap tables of `FUN_0045e3c7`.
    pub tints: Vec<[u8; 256]>,
    /// BIN entry 1, the campaign stage file.
    pub stages: Vec<u8>,
    /// `DAT_0049456c`: the signed constant block in BIN entry 10.
    pub params: Vec<u8>,
    /// `DAT_004946a8`: ITEMSP as loaded (loose `itemsp.bin` preferred), for raw Big5 names.
    pub items_raw: Vec<u8>,
    /// `DAT_00494774`: KINDSP as loaded, for class names.
    pub kinds_raw: Vec<u8>,
    /// `DAT_00494760`: item help texts (loose `itemhlsp.bin` preferred).
    pub item_help: Vec<Vec<u8>>,
    /// `DAT_004a2574`: Big5 spell school names (BIN 2).
    pub school_names: Vec<Vec<u8>>,
    /// `DAT_004a1f38`: Big5 status and buff names (BIN 2).
    pub status_names: Vec<Vec<u8>>,
    /// Big5 spell names as stored (BIN 2 records +2, `DAT_004a2560`).
    pub spell_names: Vec<Vec<u8>>,
    /// Spell help texts (BIN 9, read per spell by `FUN_0045f57c`; kept here).
    pub spell_help: Vec<Vec<u8>>,
}

impl Res {
    pub fn load(data: &Data, cfg: &Config) -> Result<Res> {
        let arcs = &data.arcs;
        // FUN_004217b9: palette entry 0 is forced to black.
        let mut master = data.dir.read("supersp.pal")?.get(..0x300).context("supersp.pal too short")?.to_vec();
        master[..3].fill(0);
        let ui = parse_indexed_strings(&arcs.read(Kind::Bin, if cfg.mainland { 6 } else { 5 })?)?;
        let ui_images = UiImages::load(arcs)?;
        let sysimg = (0..SYSIMG_RESIDENT).map(|s| ui_images.get(s + 1)).collect::<s2data::Result<Vec<_>>>()?;
        let font = Font::load(arcs, cfg.mainland)?;
        let portraits = Portraits::load(arcs)?;
        // FUN_0045daaf over the master palette, then FUN_0045e3c7.
        let mut tree = ColorTree::for_master(&master);
        tree.set_range(0x20, 0xfe);
        let tints = tint_tables(&tree);
        let stages = arcs.read(Kind::Bin, s2data::script::BIN_STAGES)?;
        let params = params_block(&arcs.read(Kind::Bin, 10)?)?;
        use s2data::tables::{load_with_override, source};
        let items_raw = load_with_override(&data.dir, arcs, source::ITEM)?;
        let kinds_raw = load_with_override(&data.dir, arcs, source::KIND)?;
        let item_help = s2data::msg::parse_string_list(&load_with_override(&data.dir, arcs, source::ITEM_HELP)?)?;
        let bin2 = arcs.read(Kind::Bin, 2)?;
        let (school_names, status_names) = bin2_names(&bin2);
        let spell_names = bin2_spell_names(&bin2);
        let spell_help = s2data::msg::parse_offset_len6(&arcs.read(Kind::Bin, 9)?)?;
        Ok(Res { master, ui, ui_images, sysimg, dat7: dat7_images(arcs)?, font, portraits, tints, stages, params, items_raw, kinds_raw, item_help, school_names, status_names, spell_names, spell_help })
    }

    /// UI image by 0-based slot (`(&DAT_00493848)[slot]` for resident ones,
    /// `FUN_00422878(slot)` for CEL 2 images loaded on demand).
    ///
    /// The original's image header holds width - 1 and height - 1, so its
    /// `*(short *)img + 1` is `width`, `(*(short *)img + 1) / 2` is `width / 2`
    /// and a bare `*(short *)img` is `width - 1` (checked against the oracle).
    pub fn ui_image(&self, slot: usize) -> Result<Image> {
        match self.sysimg.get(slot) {
            Some(img) => Ok(img.clone()),
            None => Ok(self.ui_images.get(slot + 1)?),
        }
    }

    /// `FUN_00422551(id)`: DAT 7 image, 1-based.
    pub fn dat7_image(&self, id: usize) -> Result<Image> {
        Ok(self.dat7.image(id.checked_sub(1).context("DAT 7 id 0")?)?)
    }

    /// Raw 0x26-byte ITEMSP record of 1-based item `id`.
    pub fn item_rec(&self, id: usize) -> &[u8] {
        let o = 2 + id.saturating_sub(1) * 0x26;
        self.items_raw.get(o..o + 0x26).unwrap_or(&[])
    }

    /// Big5 name of item `id` (record +2).
    pub fn item_name(&self, id: usize) -> &[u8] {
        s2data::text::cstr(self.item_rec(id).get(2..).unwrap_or(&[]))
    }

    /// Number of items (`DAT_00494788`).
    pub fn item_count(&self) -> usize {
        i16::from_le_bytes([self.items_raw[0], self.items_raw[1]]).max(0) as usize
    }

    /// Raw 0x25-byte class record (`DAT_00494580 + (kind-1)*0x25`).
    pub fn class_rec(&self, kind: usize) -> &[u8] {
        let base = self.kinds_raw.get(0x12..0x16).map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize);
        let o = base + kind.saturating_sub(1) * 0x25;
        self.kinds_raw.get(o..o + 0x25).unwrap_or(&[])
    }

    /// Big5 name of class `kind` (record +0).
    pub fn class_name(&self, kind: usize) -> &[u8] {
        s2data::text::cstr(self.class_rec(kind))
    }

    /// `DAT_00494778[i]`: KINDSP's ability name list (u32 offset at +6, i16 count at +10).
    pub fn ability_name(&self, i: usize) -> &[u8] {
        let k = &self.kinds_raw;
        let rd = |o: usize, n: usize| k.get(o..o + n).map_or(0, |b| b.iter().rev().fold(0usize, |a, &x| a << 8 | x as usize));
        let count = rd(10, 2) as i16 as i32;
        if i as i32 >= count.max(1) {
            return &[];
        }
        let mut p = rd(6, 4);
        for _ in 0..i {
            p += s2data::text::cstr(k.get(p..).unwrap_or(&[])).len() + 1;
        }
        s2data::text::cstr(k.get(p..).unwrap_or(&[]))
    }

    /// Help text of item `id` (`DAT_00494760[id]`).
    pub fn item_help_text(&self, id: usize) -> &[u8] {
        self.item_help.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    /// `DAT_0049456c[i]` as the unsigned byte the code reads.
    pub fn param(&self, i: usize) -> u8 {
        self.params.get(i).copied().unwrap_or(0)
    }
}

/// `FUN_0042422b`: the constant block starts after four per-spell byte
/// tables and the per-effect sound table (`i16 n; u8[n]`), plus 2 bytes.
fn params_block(b: &[u8]) -> Result<Vec<u8>> {
    let rd = |o: usize| -> Result<usize> {
        let s = b.get(o..o + 2).context("BIN 10 truncated")?;
        Ok(i16::from_le_bytes([s[0], s[1]]).max(0) as usize)
    };
    let n = rd(0)?;
    let m_at = 2 + 4 * n;
    let m = rd(m_at)?;
    let start = m_at + 2 + m + 2;
    match b.get(start..) {
        Some(p) => Ok(p.to_vec()),
        None => bail!("BIN 10 shorter than its tables"),
    }
}

/// `FUN_00420fab(id, pal_out, kind, ..)`: a PCX picture and its 6-bit palette.
/// `kind` 0..=4 with bases {1,1,1,0x1a,9}; negative ids are absolute entries;
/// kind 4 reads PRG.
/// The school and status names of BIN 2 as stored (`FUN_0045e4cf`,
/// `DAT_004a2574` / `DAT_004a1f38`): after the header (9 + 2 per school)
/// and the 37-byte spell records, a u16 offset table relative to its own end.
fn bin2_names(b: &[u8]) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let at = |o: usize| b.get(o).copied().unwrap_or(0) as usize;
    let (n_schools, n_spells, n_status) = (at(1), at(7), at(8));
    let strs = 9 + 2 * n_schools + 37 * n_spells;
    let base = strs + 2 * (n_schools + n_status);
    let mut names: Vec<Vec<u8>> = (0..n_schools + n_status)
        .map(|i| {
            let off = at(strs + 2 * i) | at(strs + 2 * i + 1) << 8;
            s2data::text::cstr(b.get(base + off..).unwrap_or(&[])).to_vec()
        })
        .collect();
    let status = names.split_off(n_schools);
    (names, status)
}

/// The spell names of BIN 2 as stored (37-byte records after the header, name at +2).
fn bin2_spell_names(b: &[u8]) -> Vec<Vec<u8>> {
    let at = |o: usize| b.get(o).copied().unwrap_or(0) as usize;
    let (n_schools, n_spells) = (at(1), at(7));
    let base = 9 + 2 * n_schools;
    (0..n_spells)
        .map(|i| {
            let r = base + 37 * i + 2;
            s2data::text::cstr(b.get(r..(r + 11).min(b.len())).unwrap_or(&[])).to_vec()
        })
        .collect()
}

pub fn load_picture(arcs: &Archives, id: i32, kind: u32) -> Result<(Image, Vec<u8>)> {
    const BASES: [i32; 5] = [1, 1, 1, DAT_PICTURE_BASE as i32 + 1, 9];
    let entry = if id < 0 { -id } else { BASES[kind as usize] - 1 + id };
    let archive = match kind {
        0 => Kind::Bin,
        1 => Kind::Cel,
        2 | 4 => Kind::Prg,
        _ => Kind::Dat,
    };
    let pcx = decode_pcx(&arcs.read(archive, entry as usize)?)?;
    let pal = pcx.palette6();
    Ok((pcx.image, pal))
}
