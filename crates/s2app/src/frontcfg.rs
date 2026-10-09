//! Frontend-only settings (window, scaling), kept in `opensuper2.ini` next
//! to the game data so they never touch the original `SUPERSP.CFG`.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scaling {
    /// Largest 4:3 rectangle that fits the window.
    Fit,
    /// Largest whole multiple of 640x480 that fits (sharp, may letterbox more).
    Integer,
}

#[derive(Debug, Clone)]
pub struct FrontConfig {
    pub fullscreen: bool,
    /// Window inner size in physical pixels, if remembered.
    pub window: Option<(u32, u32)>,
    pub scaling: Scaling,
    /// Palette fades run this many times faster than 60 Hz pacing (1 =
    /// frame-exact with the original under page flipping; the original's
    /// fades are untimed, so on many machines they were quicker).
    pub fade_speed: u32,
    /// `ai=enhanced`: the optional smarter enemy AI (not part of the
    /// original; off keeps the original AI exactly).
    pub enhanced_ai: bool,
    path: PathBuf,
}

impl FrontConfig {
    pub fn load(game_dir: &Path) -> FrontConfig {
        let path = game_dir.join("opensuper2.ini");
        let mut c = FrontConfig { fullscreen: false, window: None, scaling: Scaling::Fit, fade_speed: 1, enhanced_ai: false, path };
        let Ok(text) = std::fs::read_to_string(&c.path) else { return c };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim();
            match k.trim() {
                "fullscreen" => c.fullscreen = v == "1",
                "window" => {
                    c.window = v
                        .split_once(',')
                        .and_then(|(w, h)| Some((w.trim().parse().ok()?, h.trim().parse().ok()?)))
                        .filter(|&(w, h): &(u32, u32)| w >= 320 && h >= 240)
                }
                "scaling" => c.scaling = if v == "integer" { Scaling::Integer } else { Scaling::Fit },
                "fade_speed" => c.fade_speed = v.parse::<u32>().unwrap_or(1).clamp(1, 16),
                "ai" => c.enhanced_ai = v.eq_ignore_ascii_case("enhanced"),
                _ => {}
            }
        }
        c
    }

    pub fn save(&self) {
        let mut s = format!("fullscreen={}\nscaling={}\nfade_speed={}\n", self.fullscreen as u8, match self.scaling {
            Scaling::Fit => "fit",
            Scaling::Integer => "integer",
        }, self.fade_speed);
        s += if self.enhanced_ai { "ai=enhanced\n" } else { "ai=original\n" };
        if let Some((w, h)) = self.window {
            s += &format!("window={w},{h}\n");
        }
        // Best effort: a read-only game directory just means nothing is remembered.
        let _ = std::fs::write(&self.path, s);
    }
}

impl FrontConfig {
    /// Present spacing for fades, in ms.
    pub fn fade_pace_ms(&self) -> u32 {
        (16 / self.fade_speed.max(1)).max(1)
    }
}

/// Destination rectangle (x, y, w, h) of the 640x480 image in a `ww`x`wh` window.
pub fn view_rect(ww: u32, wh: u32, scaling: Scaling) -> (u32, u32, u32, u32) {
    let (vw, vh) = match scaling {
        Scaling::Integer if ww >= 640 && wh >= 480 => {
            let k = (ww / 640).min(wh / 480);
            (640 * k, 480 * k)
        }
        _ if ww * 3 >= wh * 4 => (wh * 4 / 3, wh),
        _ => (ww, ww * 3 / 4),
    };
    ((ww - vw) / 2, (wh - vh) / 2, vw, vh)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn view_rects() {
        assert_eq!(view_rect(1280, 960, Scaling::Fit), (0, 0, 1280, 960));
        assert_eq!(view_rect(1920, 1080, Scaling::Fit), (240, 0, 1440, 1080));
        assert_eq!(view_rect(1920, 1080, Scaling::Integer), (320, 60, 1280, 960));
        // Too small for 1x: falls back to fit.
        assert_eq!(view_rect(600, 400, Scaling::Integer), (33, 0, 533, 400));
    }
}
