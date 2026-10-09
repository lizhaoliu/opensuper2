//! Locating original files inside a user-supplied game directory.
//!
//! The game was written for Windows, so it refers to files with arbitrary
//! case ("supersp.pal", "SUPERSP.CFG"). Lookups here are case-insensitive so
//! the same install works on case-sensitive filesystems.

use std::fs;
use std::path::{Path, PathBuf};

use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct GameDir {
    root: PathBuf,
}

impl GameDir {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        let dir = GameDir { root };
        // The archive index lives in the palette file; without it nothing works.
        dir.resolve("supersp.pal")?;
        Ok(dir)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Resolve a '/' or '\\' separated relative path, ignoring case per component.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf> {
        let mut cur = self.root.clone();
        for part in rel.split(['/', '\\']).filter(|p| !p.is_empty()) {
            let exact = cur.join(part);
            if exact.exists() {
                cur = exact;
                continue;
            }
            let entries = fs::read_dir(&cur).map_err(|_| Error::Missing(rel.to_string()))?;
            let found = entries
                .flatten()
                .find(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(part))
                .ok_or_else(|| Error::Missing(rel.to_string()))?;
            cur = found.path();
        }
        Ok(cur)
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.resolve(rel).is_ok()
    }

    pub fn read(&self, rel: &str) -> Result<Vec<u8>> {
        let path = self.resolve(rel)?;
        fs::read(&path).map_err(|source| Error::Io { path: path.display().to_string(), source })
    }
}
