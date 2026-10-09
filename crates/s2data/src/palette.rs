//! VGA palettes. The master palette is the first 768 bytes of SUPERSP.PAL,
//! stored as 6-bit components.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette(pub [[u8; 3]; 256]);

impl Palette {
    /// Expand 6-bit VGA components (0..=63) to 8-bit the way the game does
    /// (a plain `<< 2`, so full intensity is 252, not 255).
    pub fn from_vga(b: &[u8]) -> Palette {
        let mut p = [[0u8; 3]; 256];
        for (i, c) in p.iter_mut().enumerate() {
            for (j, v) in c.iter_mut().enumerate() {
                let x = b.get(i * 3 + j).copied().unwrap_or(0) & 0x3f;
                *v = x << 2;
            }
        }
        Palette(p)
    }

    pub fn rgb(&self, index: u8) -> [u8; 3] {
        self.0[index as usize]
    }

    /// Flat RGB triplets, e.g. for a PNG PLTE chunk.
    pub fn to_rgb_bytes(&self) -> Vec<u8> {
        self.0.iter().flatten().copied().collect()
    }
}
