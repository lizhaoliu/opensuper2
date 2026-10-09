//! Readers for the data files of 超時空英雄傳說2 北方密使 (internal name "SuperSp").
//!
//! Nothing here embeds original game content: every reader works on files
//! from a user-supplied game directory. Format notes live in `docs/formats.md`.

pub mod anim;
pub mod archive;
pub mod audio;
pub mod font;
pub mod gamedir;
pub mod map;
pub mod msg;
pub mod palette;
pub mod save;
pub mod script;
pub mod sprite;
pub mod tables;
pub mod tbl_stage;
pub(crate) mod tbl_util;
pub mod text;
pub mod gfx_map;
pub mod gfx_palette;
pub mod gfx_pcx;
pub mod gfx_portrait;
pub mod gfx_ui;
pub mod gfx_unit;

pub use archive::{Archives, Kind};
pub use gamedir::GameDir;
pub use palette::Palette;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io error on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("game file not found: {0}")]
    Missing(String),
    #[error("bad data in {what}: {msg}")]
    Format { what: String, msg: String },
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn format_err(what: impl Into<String>, msg: impl Into<String>) -> Error {
    Error::Format { what: what.into(), msg: msg.into() }
}

/// Little-endian readers over a byte slice that report truncation as errors.
pub(crate) mod le {
    use super::{format_err, Result};

    pub fn u16(b: &[u8], off: usize) -> Result<u16> {
        b.get(off..off + 2)
            .map(|s| u16::from_le_bytes([s[0], s[1]]))
            .ok_or_else(|| format_err("buffer", format!("u16 read past end at {off:#x}")))
    }

    pub fn i16(b: &[u8], off: usize) -> Result<i16> {
        u16(b, off).map(|v| v as i16)
    }

    pub fn u32(b: &[u8], off: usize) -> Result<u32> {
        b.get(off..off + 4)
            .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
            .ok_or_else(|| format_err("buffer", format!("u32 read past end at {off:#x}")))
    }
}
