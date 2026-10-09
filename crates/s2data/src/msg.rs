//! Message text: UI strings, descriptions, endings, town gossip, and the
//! inline markup used in dialogue.
//!
//! All strings are kept as raw Big5 bytes so they can be drawn with
//! [`crate::font::Font`]; use [`crate::text::display`] to get UTF-8.

use crate::archive::{Archives, Kind};
use crate::text::cstr;
use crate::{format_err, le, Result};

/// BIN entry ids holding text (see docs/formats-text.md).
pub const BIN_ENDINGS: usize = 3;
pub const BIN_TOWNS: usize = 4;
pub const BIN_UI_STRINGS: usize = 5;
pub const BIN_UI_STRINGS_ALT: usize = 6;
pub const BIN_SPELL_HELP: usize = 9;
/// DAT entry used for item help when no loose ITEMHLSP.BIN exists.
pub const DAT_ITEM_HELP: usize = 3;
/// DAT entry with the installer/setup program strings.
pub const DAT_SETUP_STRINGS: usize = 8;

fn slice<'a>(b: &'a [u8], off: usize, len: usize, what: &str) -> Result<&'a [u8]> {
    b.get(off..off + len).ok_or_else(|| format_err(what, format!("string {off:#x}+{len} out of range")))
}

/// `u16 n; (u32 offset, u32 len)[n]; blob` with offsets relative to the
/// blob (UI strings, BIN 5; installer strings, DAT 8). Loader: `FUN_00424122`.
pub fn parse_indexed_strings(b: &[u8]) -> Result<Vec<Vec<u8>>> {
    let n = le::i16(b, 0)?.max(0) as usize;
    let blob = 2 + n * 8;
    (0..n)
        .map(|i| {
            let off = le::u32(b, 2 + 8 * i)? as usize;
            let len = le::u32(b, 6 + 8 * i)? as usize;
            Ok(cstr(slice(b, blob + off, len, "indexed strings")?).to_vec())
        })
        .collect()
}

/// `u16 n` followed by n NUL-terminated strings (item help, ITEMHLSP).
/// Loader: `FUN_004242f2`.
pub fn parse_string_list(b: &[u8]) -> Result<Vec<Vec<u8>>> {
    let n = le::i16(b, 0)?.max(0) as usize;
    let mut out = Vec::with_capacity(n);
    let mut p = 2;
    for _ in 0..n {
        let s = cstr(b.get(p..).ok_or_else(|| format_err("string list", "truncated"))?);
        p += s.len() + 1;
        out.push(s.to_vec());
    }
    Ok(out)
}

/// `(u32 offset, u16 len)[]` with absolute offsets; the count is implied by
/// the first offset (spell descriptions, BIN 9; loader `FUN_0045f57c`).
pub fn parse_offset_len6(b: &[u8]) -> Result<Vec<Vec<u8>>> {
    let first = le::u32(b, 0)? as usize;
    (0..first / 6)
        .map(|i| {
            let off = le::u32(b, 6 * i)? as usize;
            let len = le::u16(b, 6 * i + 4)? as usize;
            Ok(cstr(slice(b, off, len, "offset/len table")?).to_vec())
        })
        .collect()
}

/// One character's paragraph in an epilogue variant.
#[derive(Debug, Clone)]
pub struct EpilogueEntry {
    pub char_id: i16,
    pub text: Vec<u8>,
}

/// Ending texts (BIN 3, users around `FUN_0041b5dc`).
#[derive(Debug, Clone)]
pub struct Endings {
    /// Fixed pages: closing narration and staff credits ('*' = line break).
    pub pages: Vec<Vec<u8>>,
    /// Epilogue variants, each a list of per-character paragraphs.
    pub epilogues: Vec<Vec<EpilogueEntry>>,
}

impl Endings {
    /// ```text
    /// i16 n_pages, i16 n_variants
    /// (u32 offset, u16 len)[n_pages]
    /// (u32 offset, u16 count, u16 size)[n_variants]
    ///   at offset: (i16 char_id, u32 text_offset, u16 len)[count]
    /// ```
    /// All offsets are from the entry start.
    pub fn parse(b: &[u8]) -> Result<Endings> {
        let n_pages = le::i16(b, 0)?.max(0) as usize;
        let n_var = le::i16(b, 2)?.max(0) as usize;
        let pages = (0..n_pages)
            .map(|i| {
                let off = le::u32(b, 4 + 6 * i)? as usize;
                let len = le::u16(b, 8 + 6 * i)? as usize;
                Ok(cstr(slice(b, off, len, "ending page")?).to_vec())
            })
            .collect::<Result<_>>()?;
        let vt = 4 + 6 * n_pages;
        let epilogues = (0..n_var)
            .map(|v| {
                let off = le::u32(b, vt + 8 * v)? as usize;
                let count = le::u16(b, vt + 8 * v + 4)? as usize;
                (0..count)
                    .map(|k| {
                        let r = off + 8 * k;
                        let char_id = le::i16(b, r)?;
                        let t = le::u32(b, r + 2)? as usize;
                        let len = le::u16(b, r + 6)? as usize;
                        Ok(EpilogueEntry { char_id, text: cstr(slice(b, t, len, "epilogue")?).to_vec() })
                    })
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<_>>()?;
        Ok(Endings { pages, epilogues })
    }
}

/// A world-map location (BIN 4; one record per stage id). Used by the world
/// map code around `FUN_0040fd3c`, `FUN_00410d3a`, `FUN_004149ff`.
#[derive(Debug, Clone)]
pub struct Town {
    /// Position on the big world map in pixels.
    pub x: u16,
    pub y: u16,
    /// Record byte +0x0a; the world map tests it for 1.
    pub kind: u8,
    /// Item ids sold by the three shops (record bytes +0x0b..=0x0d are the counts).
    pub shops: [Vec<i16>; 3],
    pub name: Vec<u8>,
    /// Remaining NUL-terminated strings: townsfolk gossip lines.
    pub gossip: Vec<Vec<u8>>,
}

/// ```text
/// i16 n
/// (u32 offset, u16 len, u16 x, u16 y, u8 kind, u8 shop_counts[3])[n]
/// at offset: name\0, i16 items[sum(shop_counts)], gossip\0 ...
/// ```
pub fn parse_towns(b: &[u8]) -> Result<Vec<Town>> {
    let n = le::i16(b, 0)?.max(0) as usize;
    (0..n)
        .map(|i| {
            let r = 2 + 14 * i;
            let off = le::u32(b, r)? as usize;
            let len = le::u16(b, r + 4)? as usize;
            let rec = |k: usize| b.get(r + k).copied().ok_or_else(|| format_err("towns", "truncated record"));
            let blob = slice(b, off, len, "town")?;
            let name = cstr(blob).to_vec();
            let mut p = name.len() + 1;
            let mut shops: [Vec<i16>; 3] = Default::default();
            for (s, shop) in shops.iter_mut().enumerate() {
                for _ in 0..rec(0x0b + s)? {
                    shop.push(le::i16(blob, p)?);
                    p += 2;
                }
            }
            let mut gossip = Vec::new();
            while p < blob.len() {
                let s = cstr(&blob[p..]);
                p += s.len() + 1;
                if !s.is_empty() {
                    gossip.push(s.to_vec());
                }
            }
            Ok(Town { x: le::u16(b, r + 6)?, y: le::u16(b, r + 8)?, kind: rec(0x0a)?, shops, name, gossip })
        })
        .collect()
}

/// All loose text tables, loaded together for convenience.
#[derive(Debug, Clone)]
pub struct Messages {
    pub ui: Vec<Vec<u8>>,
    pub spell_help: Vec<Vec<u8>>,
    pub item_help: Vec<Vec<u8>>,
    pub endings: Endings,
    pub towns: Vec<Town>,
}

impl Messages {
    /// `item_help_file` is the loose ITEMHLSP.BIN if present; the game
    /// prefers it over the DAT copy.
    pub fn load(arcs: &Archives, item_help_file: Option<&[u8]>) -> Result<Messages> {
        let item_help = match item_help_file {
            Some(b) => parse_string_list(b)?,
            None => parse_string_list(&arcs.read(Kind::Dat, DAT_ITEM_HELP)?)?,
        };
        Ok(Messages {
            ui: parse_indexed_strings(&arcs.read(Kind::Bin, BIN_UI_STRINGS)?)?,
            spell_help: parse_offset_len6(&arcs.read(Kind::Bin, BIN_SPELL_HELP)?)?,
            item_help,
            endings: Endings::parse(&arcs.read(Kind::Bin, BIN_ENDINGS)?)?,
            towns: parse_towns(&arcs.read(Kind::Bin, BIN_TOWNS)?)?,
        })
    }

    /// UI string by index (the exe reads them as `DAT_00494768[index]`,
    /// i.e. `*(DAT_00494768 + index * 4)`).
    pub fn ui(&self, index: usize) -> &[u8] {
        self.ui.get(index).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// A piece of dialogue text after markup parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// Characters to print (raw Big5 / ASCII bytes, no markup).
    Text(Vec<u8>),
    /// '*': start a new line (ignored when already at a line start).
    Break,
    /// `@@Wcc,ss@@`: set text color and second color (hex).
    Color { color: u8, second: u8 },
    /// `@@Ddd@@`: per-character delay (hex, milliseconds per character).
    Delay(u8),
    /// `@@RW@@`: back to the default color (0x2a, 0).
    ResetColor,
    /// `@@RD@@`: back to the configured text speed.
    ResetDelay,
}

fn hex2(b: &[u8]) -> Option<u8> {
    let s = std::str::from_utf8(b.get(..2)?).ok()?;
    u8::from_str_radix(s, 16).ok()
}

/// Split dialogue text into tokens following `FUN_00418e73`.
///
/// A control block is `@@` + commands + `@@`, recognized only where the
/// typewriter is about to print. Commands: `Wcc,ss`, `Ddd`, `RW`, `RD`
/// (hex operands, read with sscanf so they are not validated; the cursor
/// always advances 6 / 3 / 2 bytes). Quirks kept from the original:
/// right after a block one character is printed unconditionally (so a
/// second block directly following prints its `@`), and an unknown command
/// ends the block with printing resuming at that byte (`R?` resumes one
/// byte later).
pub fn parse_dialogue(b: &[u8]) -> Vec<Token> {
    let b = cstr(b);
    let mut out = Vec::new();
    let mut text = Vec::new();
    let mut p = 0;
    while p < b.len() {
        let mut resume = p;
        if b[p..].starts_with(b"@@") {
            let mut q = p + 2;
            let mut cmds = Vec::new();
            loop {
                if b[q.min(b.len())..].starts_with(b"@@") {
                    resume = q + 2;
                    break;
                }
                resume = q;
                match b.get(q) {
                    None => break,
                    Some(b'W') => {
                        let c = hex2(b.get(q + 1..).unwrap_or(&[]));
                        let s = hex2(b.get(q + 4..).unwrap_or(&[]));
                        if let (Some(color), Some(second)) = (c, s) {
                            cmds.push(Token::Color { color, second });
                        }
                        q += 6;
                    }
                    Some(b'D') => {
                        if let Some(d) = hex2(b.get(q + 1..).unwrap_or(&[])) {
                            cmds.push(Token::Delay(d));
                        }
                        q += 3;
                    }
                    Some(b'R') => match b.get(q + 1) {
                        Some(b'W') => {
                            cmds.push(Token::ResetColor);
                            q += 2;
                        }
                        Some(b'D') => {
                            cmds.push(Token::ResetDelay);
                            q += 2;
                        }
                        _ => {
                            resume = q + 1;
                            break;
                        }
                    },
                    Some(_) => break,
                }
            }
            if !text.is_empty() {
                out.push(Token::Text(std::mem::take(&mut text)));
            }
            out.extend(cmds);
        }
        p = resume;
        if p < b.len() {
            take_char(b, &mut p, &mut text, &mut out);
        }
    }
    if !text.is_empty() {
        out.push(Token::Text(text));
    }
    out
}

fn take_char(b: &[u8], p: &mut usize, text: &mut Vec<u8>, out: &mut Vec<Token>) {
    let c = b[*p];
    if c == b'*' {
        if !text.is_empty() {
            out.push(Token::Text(std::mem::take(text)));
        }
        out.push(Token::Break);
        *p += 1;
    } else if c < 0x81 {
        text.push(c);
        *p += 1;
    } else {
        text.extend_from_slice(&b[*p..(*p + 2).min(b.len())]);
        *p += 2;
    }
}

/// Dialogue box geometry used by `FUN_004188ca` / `FUN_00418e73`,
/// relative to the box origin (`DAT_0048e294`, `DAT_0048e298`).
pub mod layout {
    /// Text starts here.
    pub const TEXT_X: isize = 0x8f;
    pub const TEXT_Y: isize = 0x1d;
    /// Line height.
    pub const LINE_H: isize = 0x19;
    /// Wrap once the pen reaches this x.
    pub const WRAP_X: isize = 0x23f;
    /// A page is full once the pen is at/after this y and x (then wait for a key).
    pub const PAGE_Y: isize = 0xb2;
    /// "More" arrow (UI image 0xec) position.
    pub const MORE_X: isize = 0x244;
    pub const MORE_Y: isize = 0xb7;
    /// Speaker name: centered on x = 0x4a, at y = 0xa8, drawn with `FUN_0041f748`
    /// in color 0x36 outlined with 0.
    pub const NAME_CX: isize = 0x4a;
    pub const NAME_Y: isize = 0xa8;
    /// Portrait: centered on x = 0x4a at y = 0x1d.
    pub const PORTRAIT_Y: isize = 0x1d;
    /// Default text color / second color.
    pub const TEXT_COLOR: u8 = 0x2a;
    /// Box origin used when the speaker is not on the map.
    pub const DEFAULT_BOX: (isize, isize) = (6, 0x7d);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_blocks_are_parsed() {
        let t = parse_dialogue(b"ab@@W1f,02D10@@cd*ef@@RWRD@@g\0junk");
        assert_eq!(
            t,
            vec![
                Token::Text(b"ab".to_vec()),
                Token::Color { color: 0x1f, second: 2 },
                Token::Delay(0x10),
                Token::Text(b"cd".to_vec()),
                Token::Break,
                Token::Text(b"ef".to_vec()),
                Token::ResetColor,
                Token::ResetDelay,
                Token::Text(b"g".to_vec()),
            ]
        );
    }

    #[test]
    fn bad_block_falls_back_to_text() {
        assert_eq!(parse_dialogue(b"@@Xyz"), vec![Token::Text(b"Xyz".to_vec())]);
        assert_eq!(parse_dialogue(b"@@RQa"), vec![Token::Text(b"Qa".to_vec())]);
    }

    #[test]
    fn char_after_block_is_always_printed() {
        // The '@' right after the first block is printed, so the second block
        // is never recognized; a later bare "@@" swallows itself.
        assert_eq!(
            parse_dialogue(b"@@RW@@@@RD@@x"),
            vec![Token::ResetColor, Token::Text(b"@@RD".to_vec()), Token::Text(b"x".to_vec())]
        );
    }

    #[test]
    fn indexed_strings_roundtrip() {
        // two strings "a" and "bc"
        let mut b = vec![2, 0];
        b.extend_from_slice(&0u32.to_le_bytes());
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&3u32.to_le_bytes());
        b.extend_from_slice(b"a\0bc\0");
        assert_eq!(parse_indexed_strings(&b).unwrap(), vec![b"a".to_vec(), b"bc".to_vec()]);
    }
}
