//! Shared helpers for the s2tool binaries.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result};
use s2data::sprite::Image;
use s2data::{GameDir, Palette};

pub fn master_palette(dir: &GameDir) -> Result<Palette> {
    Ok(Palette::from_vga(&dir.read("supersp.pal")?))
}

/// Write an indexed image as PNG; with `transparent`, index 0 is see-through.
pub fn write_png(path: &Path, img: &Image, pal: &Palette, transparent: bool) -> Result<()> {
    let w = BufWriter::new(File::create(path).with_context(|| format!("creating {}", path.display()))?);
    let mut enc = png::Encoder::new(w, img.width as u32, img.height as u32);
    enc.set_color(png::ColorType::Indexed);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_palette(pal.to_rgb_bytes());
    if transparent {
        enc.set_trns(vec![0u8]);
    }
    enc.write_header()?.write_image_data(&img.pixels)?;
    Ok(())
}
