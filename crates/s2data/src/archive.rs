//! The four big archives (SUPERSP.BIN/.CEL/.PRG/.DAT) and their index.
//!
//! Every archive starts with a 28-byte header:
//!
//! ```text
//! u32  data_len        file length - 28
//! [4]  "U&J\0"
//! [10] "SuperSp_X\0"   X = B (bin), C (cel), P (prg), W (dat)
//! [4]  version         "1.1\0" / "1.3\0"
//! [2]  padding
//! u32  entry count
//! ```
//!
//! The game ignores the in-file entry list and instead uses an index appended
//! to SUPERSP.PAL after the 768-byte palette (see `FUN_004217b9`). That index
//! is lightly obfuscated: each count has a constant added and each
//! (offset, size) pair of table *k* has `base_k + i` added to both fields.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::Mutex;

use crate::{format_err, le, GameDir, Result};

pub const HEADER_LEN: usize = 28;
pub const MAGIC: &[u8; 4] = b"U&J\0";

/// Archive selector, numbered like the game's loader (`FUN_0046dedd`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Bin = 0,
    Cel = 1,
    Prg = 2,
    Dat = 3,
}

impl Kind {
    pub const ALL: [Kind; 4] = [Kind::Bin, Kind::Cel, Kind::Prg, Kind::Dat];

    pub fn name(self) -> &'static str {
        match self {
            Kind::Bin => "bin",
            Kind::Cel => "cel",
            Kind::Prg => "prg",
            Kind::Dat => "dat",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub offset: u32,
    pub size: u32,
}

/// Decoded contents of the index stored in SUPERSP.PAL.
#[derive(Debug, Clone)]
pub struct Index {
    pub cel: Vec<Span>,
    pub bin: Vec<Span>,
    /// DAT layout of disc 1 (the game uses it when the disc id global is 0).
    pub dat_disc1: Vec<Span>,
    pub prg: Vec<Span>,
    /// DAT layout of disc 2.
    pub dat_disc2: Vec<Span>,
    /// Expected `data_len` header field of each archive (cel, bin, dat1, prg, dat2).
    pub data_lens: [u32; 5],
}

impl Index {
    /// Parse the index from a full SUPERSP.PAL file.
    pub fn parse(pal_file: &[u8]) -> Result<Index> {
        const BASES: [u32; 5] = [0x38, 0x0b, 0x08, 0x0a, 0x14];
        let words = pal_file
            .get(0x300..)
            .ok_or_else(|| format_err("supersp.pal", "too short for index"))?;
        let word = |i: usize| le::u32(words, i * 4);
        let mut counts = [0usize; 5];
        let mut data_lens = [0u32; 5];
        for k in 0..5 {
            counts[k] = word(k)?.wrapping_sub(BASES[k]) as usize;
            data_lens[k] = word(5 + k)?.wrapping_sub(BASES[k]);
        }
        let mut pos = 10;
        let mut tables: Vec<Vec<Span>> = Vec::with_capacity(5);
        for k in 0..5 {
            // The game's loop for the disc-2 DAT table iterates with the disc-1
            // count; both are 41 in the shipped data so this is equivalent.
            let mut t = Vec::with_capacity(counts[k]);
            for i in 0..counts[k] {
                let key = BASES[k] + i as u32;
                t.push(Span {
                    offset: word(pos + 2 * i)?.wrapping_sub(key),
                    size: word(pos + 2 * i + 1)?.wrapping_sub(key),
                });
            }
            pos += 2 * counts[k];
            tables.push(t);
        }
        let mut it = tables.into_iter();
        Ok(Index {
            cel: it.next().unwrap(),
            bin: it.next().unwrap(),
            dat_disc1: it.next().unwrap(),
            prg: it.next().unwrap(),
            dat_disc2: it.next().unwrap(),
            data_lens,
        })
    }
}

struct ArchiveFile {
    path: PathBuf,
    file: Mutex<File>,
    len: u64,
}

impl ArchiveFile {
    fn open(dir: &GameDir, rel: &str) -> Result<Self> {
        let path = dir.resolve(rel)?;
        let file = File::open(&path).map_err(|source| crate::Error::Io { path: path.display().to_string(), source })?;
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        Ok(ArchiveFile { path, file: Mutex::new(file), len })
    }

    fn read_span(&self, span: Span) -> Result<Vec<u8>> {
        let io = |source| crate::Error::Io { path: self.path.display().to_string(), source };
        if span.offset as u64 + span.size as u64 > self.len {
            return Err(format_err(self.path.display().to_string(), format!("span {span:?} beyond file end")));
        }
        let mut f = self.file.lock().unwrap();
        f.seek(SeekFrom::Start(span.offset as u64)).map_err(io)?;
        let mut buf = vec![0; span.size as usize];
        f.read_exact(&mut buf).map_err(io)?;
        Ok(buf)
    }
}

/// Open handles on all archives plus the decoded index.
pub struct Archives {
    pub index: Index,
    bin: ArchiveFile,
    cel: ArchiveFile,
    prg: ArchiveFile,
    dat: ArchiveFile,
    dat_disc2: bool,
}

impl Archives {
    pub fn open(dir: &GameDir) -> Result<Self> {
        let index = Index::parse(&dir.read("supersp.pal")?)?;
        let bin = ArchiveFile::open(dir, "supersp.bin")?;
        let cel = ArchiveFile::open(dir, "supersp.cel")?;
        // Installed copies keep a PRG next to the exe (the game's "HD install"
        // path); a disc layout only has it under SUPERSP\.
        let prg = ArchiveFile::open(dir, "supersp.prg").or_else(|_| ArchiveFile::open(dir, "supersp/supersp.prg"))?;
        let dat = ArchiveFile::open(dir, "supersp/supersp.dat").or_else(|_| ArchiveFile::open(dir, "supersp.dat"))?;
        // Pick whichever DAT layout matches the file we actually have.
        let dat_disc2 = dat.len == index.data_lens[4] as u64 + HEADER_LEN as u64;
        Ok(Archives { index, bin, cel, prg, dat, dat_disc2 })
    }

    fn table(&self, kind: Kind) -> &[Span] {
        match kind {
            Kind::Bin => &self.index.bin,
            Kind::Cel => &self.index.cel,
            Kind::Prg => &self.index.prg,
            Kind::Dat if self.dat_disc2 => &self.index.dat_disc2,
            Kind::Dat => &self.index.dat_disc1,
        }
    }

    pub fn count(&self, kind: Kind) -> usize {
        self.table(kind).len()
    }

    pub fn span(&self, kind: Kind, id: usize) -> Result<Span> {
        let t = self.table(kind);
        id.checked_sub(1)
            .and_then(|i| t.get(i).copied())
            .ok_or_else(|| format_err(kind.name(), format!("entry {id} out of range 1..={}", t.len())))
    }

    /// Read a whole top-level entry. `id` is 1-based, as in the game.
    pub fn read(&self, kind: Kind, id: usize) -> Result<Vec<u8>> {
        let span = self.span(kind, id)?;
        self.file(kind).read_span(span)
    }

    /// Read `size` bytes at `offset` inside top-level entry `id`.
    pub fn read_within(&self, kind: Kind, id: usize, offset: u32, size: u32) -> Result<Vec<u8>> {
        let span = self.span(kind, id)?;
        if offset as u64 + size as u64 > span.size as u64 {
            return Err(format_err(kind.name(), format!("sub-read {offset:#x}+{size:#x} outside entry {id}")));
        }
        self.file(kind).read_span(Span { offset: span.offset + offset, size })
    }

    fn file(&self, kind: Kind) -> &ArchiveFile {
        match kind {
            Kind::Bin => &self.bin,
            Kind::Cel => &self.cel,
            Kind::Prg => &self.prg,
            Kind::Dat => &self.dat,
        }
    }
}

/// Parse the `u16 count; (u32 offset, u32 size)[count]` table that starts many
/// entries (offsets are relative to the start of the table).
pub fn offset_table(b: &[u8]) -> Result<Vec<Span>> {
    let n = le::u16(b, 0)? as usize;
    (0..n)
        .map(|i| Ok(Span { offset: le::u32(b, 2 + 8 * i)?, size: le::u32(b, 6 + 8 * i)? }))
        .collect()
}

/// Validate the 28-byte archive header and return (type letter, version, count).
pub fn parse_header(b: &[u8]) -> Result<(u8, String, u32)> {
    if b.len() < HEADER_LEN || &b[4..8] != MAGIC || &b[8..16] != b"SuperSp_" {
        return Err(format_err("archive", "missing U&J SuperSp_ header"));
    }
    let ver = String::from_utf8_lossy(&b[18..21]).into_owned();
    Ok((b[16], ver, le::u32(b, 24)?))
}
