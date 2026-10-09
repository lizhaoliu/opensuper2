//! Small bounds-checked record reader shared by the table modules.

use crate::{format_err, Result};

/// A view over one fixed-size record with typed little-endian getters.
#[derive(Clone, Copy)]
pub(crate) struct Rec<'a> {
    pub b: &'a [u8],
    what: &'static str,
}

impl<'a> Rec<'a> {
    pub fn new(b: &'a [u8], what: &'static str) -> Rec<'a> {
        Rec { b, what }
    }

    /// Sub-record `[off, off + len)` of a larger buffer.
    pub fn at(b: &'a [u8], off: usize, len: usize, what: &'static str) -> Result<Rec<'a>> {
        b.get(off..off + len)
            .map(|b| Rec { b, what })
            .ok_or_else(|| format_err(what, format!("record {off:#x}+{len:#x} past end ({:#x})", b.len())))
    }

    fn get(&self, off: usize, n: usize) -> Result<&'a [u8]> {
        self.b.get(off..off + n).ok_or_else(|| format_err(self.what, format!("field at {off:#x} past end")))
    }

    pub fn u8(&self, off: usize) -> Result<u8> {
        Ok(self.get(off, 1)?[0])
    }

    pub fn i8(&self, off: usize) -> Result<i8> {
        Ok(self.get(off, 1)?[0] as i8)
    }

    pub fn u16(&self, off: usize) -> Result<u16> {
        let s = self.get(off, 2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }

    pub fn i16(&self, off: usize) -> Result<i16> {
        self.u16(off).map(|v| v as i16)
    }

    pub fn u32(&self, off: usize) -> Result<u32> {
        let s = self.get(off, 4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }

    pub fn i32(&self, off: usize) -> Result<i32> {
        self.u32(off).map(|v| v as i32)
    }

    pub fn bytes(&self, off: usize, n: usize) -> Result<&'a [u8]> {
        self.get(off, n)
    }

    /// Fixed-width Big5 string field (NUL-terminated within `n` bytes).
    pub fn str(&self, off: usize, n: usize) -> Result<String> {
        Ok(crate::text::big5(self.get(off, n)?))
    }

    /// The same field as stored (Big5 bytes up to the NUL), for the fonts.
    pub fn raw_str(&self, off: usize, n: usize) -> Result<Vec<u8>> {
        let s = self.get(off, n)?;
        Ok(s[..s.iter().position(|&c| c == 0).unwrap_or(s.len())].to_vec())
    }
}

/// Read a NUL-terminated Big5 string at `off`, returning it and the offset after the NUL.
pub(crate) fn cstr_at(b: &[u8], off: usize, what: &'static str) -> Result<(String, usize)> {
    let rest = b.get(off..).ok_or_else(|| format_err(what, format!("string at {off:#x} past end")))?;
    let end = rest.iter().position(|&c| c == 0).ok_or_else(|| format_err(what, "unterminated string"))?;
    Ok((crate::text::big5(&rest[..end]), off + end + 1))
}

/// `n` consecutive NUL-terminated strings starting at `off`.
pub(crate) fn cstr_list(b: &[u8], mut off: usize, n: usize, what: &'static str) -> Result<Vec<String>> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let (s, next) = cstr_at(b, off, what)?;
        out.push(s);
        off = next;
    }
    Ok(out)
}
