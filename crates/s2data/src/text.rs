//! Text in the data files is Big5 (Traditional Chinese, code page 950).
//!
//! The game's own text walkers (e.g. `FUN_0041f748`) do not validate Big5:
//! any byte below 0x81 is a one-byte character, anything else starts a
//! two-byte character, and a string ends at NUL or '\n'. [`chars`] follows
//! the same rule so layout matches the original exactly.

/// Decode a Big5 byte string, stopping at the first NUL.
pub fn big5(b: &[u8]) -> String {
    let (s, _, _) = encoding_rs::BIG5.decode(cstr(b));
    s.into_owned()
}

/// The bytes up to (not including) the first NUL.
pub fn cstr(b: &[u8]) -> &[u8] {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    &b[..end]
}

/// One character as the game's renderer sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ch {
    /// A single byte below 0x81 (ASCII and control bytes).
    Byte(u8),
    /// A two-byte Big5 code (lead byte >= 0x81).
    Wide(u8, u8),
}

/// Split game text into characters, stopping at NUL or '\n' like the game.
pub fn chars(b: &[u8]) -> Vec<Ch> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c == 0 || c == b'\n' {
            break;
        }
        if c < 0x81 {
            out.push(Ch::Byte(c));
            i += 1;
        } else {
            out.push(Ch::Wide(c, b.get(i + 1).copied().unwrap_or(0)));
            i += 2;
        }
    }
    out
}

/// Decode for display in dumps: Big5 to UTF-8 with the game's '*' line
/// break shown as a newline.
pub fn display(b: &[u8]) -> String {
    let mut s = String::new();
    for ch in chars(b) {
        match ch {
            Ch::Byte(b'*') => s.push('\n'),
            Ch::Byte(c) => s.push(c as char),
            Ch::Wide(hi, lo) => {
                let pair = [hi, lo];
                let (d, _, _) = encoding_rs::BIG5.decode(&pair);
                s.push_str(&d);
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_like_the_game() {
        // "A" + U+4E00 (一 = A4 40 in Big5) + '*' + stop at '\n'
        let b = [b'A', 0xa4, 0x40, b'*', b'\n', b'B'];
        assert_eq!(chars(&b), vec![Ch::Byte(b'A'), Ch::Wide(0xa4, 0x40), Ch::Byte(b'*')]);
        assert_eq!(display(&b), "A一\n");
    }

    #[test]
    fn cstr_stops_at_nul() {
        assert_eq!(cstr(b"ab\0cd"), b"ab");
        assert_eq!(big5(&[0xa4, 0x40, 0, 0xa4]), "一");
    }
}
