//! UI images: cursors, markers, projectiles, icons and window frames.
//!
//! The game keeps one id space (`FUN_00421f14`, `FUN_00422897`):
//!
//! * ids `1..=142`: DAT entry 5, raw frames (cursors, move/attack diamonds,
//!   16-direction arrows and spell projectiles, ...). The loader checks the
//!   count is exactly 0x8e.
//! * ids `143..`: CEL entry 2, RLE frames (653 images: panels, windows,
//!   buttons, 32x32 skill/item icons, ...). The loader checks 0x28d entries;
//!   ids 143..=200 are preloaded at start-up, the rest on demand.
//!
//! Extra full-screen UI art lives in DAT entry 7 (flagged frames, see
//! [`dat7_images`]).

use crate::sprite::{FrameKind, Image, ImageBank};
use crate::{format_err, Archives, Kind, Result};

pub const DAT_UI_COUNT: usize = 0x8e;

pub struct UiImages {
    small: ImageBank,
    large: ImageBank,
}

impl UiImages {
    pub fn load(arcs: &Archives) -> Result<UiImages> {
        let small = ImageBank::parse(arcs.read(Kind::Dat, 5)?, FrameKind::Raw)?;
        let large = ImageBank::parse(arcs.read(Kind::Cel, 2)?, FrameKind::Rle)?;
        if small.len() != DAT_UI_COUNT {
            return Err(format_err("ui images", format!("DAT 5 has {} images, expected 142", small.len())));
        }
        Ok(UiImages { small, large })
    }

    /// Highest valid id.
    pub fn max_id(&self) -> usize {
        DAT_UI_COUNT + self.large.len()
    }

    /// Image by game id (1-based).
    pub fn get(&self, id: usize) -> Result<Image> {
        match id {
            1..=DAT_UI_COUNT => self.small.image(id - 1),
            _ if id > DAT_UI_COUNT && id <= self.max_id() => self.large.image(id - DAT_UI_COUNT - 1),
            _ => Err(format_err("ui images", format!("id {id} out of range"))),
        }
    }
}

/// DAT entry 7: 11 flagged images (status screen, list windows, buttons, a
/// 640x480 previous-game title); loaded by id with `FUN_00422551`/`FUN_0042267c`.
pub fn dat7_images(arcs: &Archives) -> Result<ImageBank> {
    ImageBank::parse(arcs.read(Kind::Dat, 7)?, FrameKind::Flagged)
}

/// DAT entry 10: 14 RLE images that no code path in v1.4 loads (leftovers;
/// their palette is not the master one and is unknown).
pub fn dat10_images(arcs: &Archives) -> Result<ImageBank> {
    ImageBank::parse(arcs.read(Kind::Dat, 10)?, FrameKind::Rle)
}
