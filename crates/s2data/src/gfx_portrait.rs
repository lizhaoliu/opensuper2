//! Character portraits: CEL entry 3, 334 flagged frames = 167 characters x 2.
//!
//! For character id `n` (1-based):
//! * entry `2n-2` is the 100x124 portrait used in dialogue (`FUN_00422353`);
//! * entry `2n-1` is the 92x111 portrait used on battle/status panels
//!   (`FUN_0043fa24`/`FUN_0043fc56` via `FUN_0042227c`).

use crate::sprite::{FrameKind, Image, ImageBank};
use crate::{format_err, Archives, Kind, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortraitKind {
    Dialogue,
    Battle,
}

pub struct Portraits {
    bank: ImageBank,
}

impl Portraits {
    pub fn load(arcs: &Archives) -> Result<Portraits> {
        Ok(Portraits { bank: ImageBank::parse(arcs.read(Kind::Cel, 3)?, FrameKind::Flagged)? })
    }

    pub fn count(&self) -> usize {
        self.bank.len() / 2
    }

    pub fn get(&self, id: usize, kind: PortraitKind) -> Result<Image> {
        if id == 0 || id > self.count() {
            return Err(format_err("portraits", format!("character {id} out of range")));
        }
        self.bank.image(match kind {
            PortraitKind::Dialogue => 2 * id - 2,
            PortraitKind::Battle => 2 * id - 1,
        })
    }
}
