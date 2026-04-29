use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileSource {
    Binary(PathBuf),
    HexImport(PathBuf),
    New,
}

pub struct Buffer {
    data: Vec<u8>,
    modified: HashSet<usize>,
    dirty: bool,
    source: FileSource,
}

impl Buffer {
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            modified: HashSet::new(),
            dirty: false,
            source: FileSource::New,
        }
    }

    pub fn from_file(path: &Path) -> Result<Self> {
        let data = fs::read(path)
            .with_context(|| format!("Failed to read file: {}", path.display()))?;
        Ok(Self {
            data,
            modified: HashSet::new(),
            dirty: false,
            source: FileSource::Binary(path.to_path_buf()),
        })
    }

    pub fn from_hex_import(path: &Path) -> Result<Self> {
        let data = crate::import::parse_hex_file(path)?;
        Ok(Self {
            data,
            modified: HashSet::new(),
            dirty: false,
            source: FileSource::HexImport(path.to_path_buf()),
        })
    }

    pub fn get_byte(&self, offset: usize) -> Option<u8> {
        self.data.get(offset).copied()
    }

    pub fn get_range(&self, offset: usize, len: usize) -> &[u8] {
        let start = offset.min(self.data.len());
        let end = (offset + len).min(self.data.len());
        &self.data[start..end]
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn set_byte(&mut self, offset: usize, value: u8) {
        if let Some(byte) = self.data.get_mut(offset) {
            if *byte != value {
                *byte = value;
                self.modified.insert(offset);
                self.dirty = true;
            }
        }
    }

    pub fn insert_byte(&mut self, offset: usize, value: u8) {
        let offset = offset.min(self.data.len());
        self.data.insert(offset, value);

        // 先平移已有的 modified 索引
        let mut new_modified = HashSet::new();
        for &idx in &self.modified {
            if idx >= offset {
                new_modified.insert(idx + 1);
            } else {
                new_modified.insert(idx);
            }
        }
        // 再标记新插入的字节
        new_modified.insert(offset);
        self.modified = new_modified;

        self.dirty = true;
    }

    pub fn remove_byte(&mut self, offset: usize) -> Option<u8> {
        if offset >= self.data.len() {
            return None;
        }
        let value = self.data.remove(offset);
        // Shift modified indices
        let mut new_modified = HashSet::new();
        for &idx in &self.modified {
            if idx == offset {
                continue;
            } else if idx > offset {
                new_modified.insert(idx - 1);
            } else {
                new_modified.insert(idx);
            }
        }
        self.modified = new_modified;
        self.dirty = true;
        Some(value)
    }

    pub fn is_modified(&self, offset: usize) -> bool {
        self.modified.contains(&offset)
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn save(&self) -> Result<()> {
        match &self.source {
            FileSource::Binary(path) => {
                fs::write(path, &self.data)
                    .with_context(|| format!("Failed to save file: {}", path.display()))?;
            }
            FileSource::HexImport(path) => {
                let bin_path = Self::infer_bin_path(path);
                fs::write(&bin_path, &self.data)
                    .with_context(|| format!("Failed to save file: {}", bin_path.display()))?;
            }
            FileSource::New => {
                anyhow::bail!("Cannot save buffer with no file path. Use save_as instead.");
            }
        }
        Ok(())
    }

    fn infer_bin_path(path: &Path) -> PathBuf {
        if let Some(stem) = path.file_stem() {
            path.with_file_name(stem).with_extension("bin")
        } else {
            path.with_extension("bin")
        }
    }

    pub fn save_as(&self, path: &Path) -> Result<()> {
        fs::write(path, &self.data)
            .with_context(|| format!("Failed to save file: {}", path.display()))?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn source(&self) -> &FileSource {
        &self.source
    }

    pub fn set_source(&mut self, source: FileSource) {
        self.source = source;
    }

    pub fn file_name(&self) -> Option<String> {
        match &self.source {
            FileSource::Binary(path) | FileSource::HexImport(path) => {
                path.file_name().map(|n| n.to_string_lossy().to_string())
            }
            FileSource::New => None,
        }
    }
}

impl Default for Buffer {
    fn default() -> Self {
        Self::new()
    }
}
