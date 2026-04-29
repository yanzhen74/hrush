#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use memmap2::Mmap;

pub struct LargeFileBuffer {
    mmap: Mmap,
    overlay: BTreeMap<usize, u8>,
    inserts: BTreeMap<usize, Vec<u8>>,
    file_path: PathBuf,
    original_len: usize,
}

impl LargeFileBuffer {
    pub fn open(path: &Path) -> Result<Self> {
        let file = File::open(path)
            .with_context(|| format!("Failed to open file: {}", path.display()))?;
        let mmap = unsafe {
            Mmap::map(&file)
                .with_context(|| format!("Failed to mmap file: {}", path.display()))?
        };
        let original_len = mmap.len();
        Ok(Self {
            mmap,
            overlay: BTreeMap::new(),
            inserts: BTreeMap::new(),
            file_path: path.to_path_buf(),
            original_len,
        })
    }

    pub fn get_byte(&self, offset: usize) -> Option<u8> {
        if let Some(&value) = self.overlay.get(&offset) {
            return Some(value);
        }
        if offset < self.original_len {
            return Some(self.mmap[offset]);
        }
        // Check inserts that might cover this offset
        let effective_offset = self.effective_offset(offset);
        if effective_offset < self.original_len {
            return Some(self.mmap[effective_offset]);
        }
        None
    }

    pub fn get_range(&self, offset: usize, len: usize) -> Vec<u8> {
        let mut result = Vec::with_capacity(len);
        for i in 0..len {
            if let Some(byte) = self.get_byte(offset + i) {
                result.push(byte);
            } else {
                break;
            }
        }
        result
    }

    pub fn set_byte(&mut self, offset: usize, value: u8) {
        if offset < self.original_len {
            self.overlay.insert(offset, value);
        }
    }

    pub fn len(&self) -> usize {
        let insert_count: usize = self.inserts.values().map(|v| v.len()).sum();
        self.original_len + insert_count
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&self.file_path)
    }

    pub fn save_as(&self, path: &Path) -> Result<()> {
        self.save_to(path)
    }

    fn save_to(&self, path: &Path) -> Result<()> {
        let temp_path = path.with_extension("tmp");
        let mut temp_file = File::create(&temp_path)
            .with_context(|| format!("Failed to create temp file: {}", temp_path.display()))?;

        for i in 0..self.original_len {
            let byte = if let Some(&value) = self.overlay.get(&i) {
                value
            } else {
                self.mmap[i]
            };
            temp_file.write_all(&[byte])
                .with_context(|| "Failed to write to temp file")?;
        }

        // Append inserts that are after the end
        for (offset, data) in &self.inserts {
            if *offset >= self.original_len {
                temp_file.write_all(data)
                    .with_context(|| "Failed to write insert data")?;
            }
        }

        temp_file.flush()
            .with_context(|| "Failed to flush temp file")?;
        drop(temp_file);

        fs::rename(&temp_path, path)
            .with_context(|| format!("Failed to rename temp file to: {}", path.display()))?;

        Ok(())
    }

    fn effective_offset(&self, offset: usize) -> usize {
        // Simple approximation: count how many insertions happen before this offset
        let mut shift = 0usize;
        for (&ins_offset, data) in &self.inserts {
            if ins_offset <= offset.saturating_sub(shift) {
                shift += data.len();
            } else {
                break;
            }
        }
        offset.saturating_sub(shift)
    }
}
