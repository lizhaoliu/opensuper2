//! Short notices of frontend settings (filter, smooth motion) drawn over the
//! top of the frame in the game's own font for a moment.

use std::time::{Duration, Instant};

use s2data::font::{Font, Mode, Style};
use s2data::sprite::Image;
use s2data::Archives;
use s2engine::sys::SCREEN_W;

const SHOWN: Duration = Duration::from_millis(1600);
const TEXT: u8 = 1;
const EDGE: u8 = 2;

pub struct Painter {
    font: Option<Font>,
}

pub struct Toast {
    img: Image,
    pub until: Instant,
}

impl Painter {
    pub fn new(arcs: &Archives) -> Painter {
        Painter { font: Font::load(arcs, false).ok() }
    }

    pub fn make(&self, text: &str) -> Option<Toast> {
        let font = self.font.as_ref()?;
        let (big5, _, _) = encoding_rs::BIG5.encode(text);
        let style = Style::new(TEXT, EDGE, Mode::Outline);
        let mut img = Image::new(SCREEN_W, 26);
        let end = font.draw_line_small(&mut img, 10, 4, &big5, style).clamp(0, SCREEN_W as isize - 10) as usize;
        // Crop to the text plus a margin.
        let w = end + 10;
        let mut out = Image::new(w, img.height);
        for y in 0..img.height {
            out.pixels[y * w..(y + 1) * w].copy_from_slice(&img.pixels[y * SCREEN_W..y * SCREEN_W + w]);
        }
        Some(Toast { img: out, until: Instant::now() + SHOWN })
    }
}

impl Toast {
    /// Draw onto a 640-wide 0RGB frame: a darkened box with the text.
    pub fn draw(&self, rgb: &mut [u32]) {
        let x0 = SCREEN_W.saturating_sub(self.img.width) / 2;
        let y0 = 8;
        for y in 0..self.img.height {
            let row = (y0 + y) * SCREEN_W;
            for x in 0..self.img.width {
                let Some(d) = rgb.get_mut(row + x0 + x) else { return };
                *d = match self.img.pixels[y * self.img.width + x] {
                    TEXT => 0xf8f8f8,
                    EDGE => 0x101010,
                    _ => (*d >> 2) & 0x3f3f3f,
                };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Needs the game files (`S2_GAME_DIR`); skipped without them.
    #[test]
    fn notice_draws_text() {
        let Some(dir) = std::env::var_os("S2_GAME_DIR") else { return };
        let data = s2engine::Data::open(std::path::Path::new(&dir)).unwrap();
        let t = Painter::new(&data.arcs).make("畫面：xBR 平滑").expect("font");
        let mut rgb = vec![0x808080u32; SCREEN_W * 480];
        t.draw(&mut rgb);
        assert!(rgb.iter().filter(|&&c| c == 0xf8f8f8).count() > 100);
        assert!(rgb.iter().filter(|&&c| c == 0x202020).count() > 1000);
        if let Some(out) = std::env::var_os("S2_TOAST_PNG") {
            let mut enc = png::Encoder::new(std::fs::File::create(out).unwrap(), SCREEN_W as u32, 40);
            enc.set_color(png::ColorType::Rgb);
            let bytes: Vec<u8> = rgb[..SCREEN_W * 40].iter().flat_map(|c| [(c >> 16) as u8, (c >> 8) as u8, *c as u8]).collect();
            enc.write_header().unwrap().write_image_data(&bytes).unwrap();
        }
    }
}
