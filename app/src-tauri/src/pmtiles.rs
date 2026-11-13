//! Minimal reader for PMTiles v3 archives on local disk.
//!
//! A PMTiles file is one header, a root directory, optional leaf
//! directories and the tile data. Tiles are addressed by a Hilbert-curve
//! tile id; directories map id ranges to byte ranges. See
//! https://github.com/protomaps/PMTiles/blob/main/spec/v3/spec.md

use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use memmap2::Mmap;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy)]
struct Entry {
    tile_id: u64,
    offset: u64,
    length: u32,
    run_length: u32,
}

#[derive(Debug, Clone)]
pub struct Header {
    root_offset: u64,
    root_length: u64,
    metadata_offset: u64,
    metadata_length: u64,
    leaf_offset: u64,
    data_offset: u64,
    internal_compression: u8,
    tile_compression: u8,
    pub min_zoom: u8,
    pub max_zoom: u8,
    /// [west, south, east, north] in degrees
    pub bounds: [f64; 4],
}

pub struct PmTiles {
    #[allow(dead_code)]
    pub path: PathBuf,
    mmap: Mmap,
    pub header: Header,
    root: Vec<Entry>,
    leaves: Mutex<HashMap<u64, Arc<Vec<Entry>>>>,
}

fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

fn i32_at(b: &[u8], o: usize) -> i32 {
    i32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn decompress(data: &[u8], compression: u8) -> Result<Vec<u8>> {
    match compression {
        0 | 1 => Ok(data.to_vec()), // unknown / none
        2 => {
            let mut out = Vec::with_capacity(data.len() * 3);
            GzDecoder::new(data).read_to_end(&mut out)?;
            Ok(out)
        }
        c => bail!("unsupported compression {c}"),
    }
}

fn read_varint(b: &[u8], pos: &mut usize) -> Result<u64> {
    let mut v: u64 = 0;
    let mut shift = 0;
    loop {
        let byte = *b.get(*pos).context("truncated directory")?;
        *pos += 1;
        v |= ((byte & 0x7f) as u64) << shift;
        if byte & 0x80 == 0 {
            return Ok(v);
        }
        shift += 7;
        if shift > 63 {
            bail!("varint too long");
        }
    }
}

fn parse_directory(raw: &[u8]) -> Result<Vec<Entry>> {
    let mut pos = 0;
    let n = read_varint(raw, &mut pos)? as usize;
    let mut entries = vec![Entry { tile_id: 0, offset: 0, length: 0, run_length: 0 }; n];
    let mut last = 0u64;
    for e in entries.iter_mut() {
        last += read_varint(raw, &mut pos)?;
        e.tile_id = last;
    }
    for e in entries.iter_mut() {
        e.run_length = read_varint(raw, &mut pos)? as u32;
    }
    for e in entries.iter_mut() {
        e.length = read_varint(raw, &mut pos)? as u32;
    }
    for i in 0..n {
        let v = read_varint(raw, &mut pos)?;
        entries[i].offset = if v == 0 && i > 0 {
            entries[i - 1].offset + entries[i - 1].length as u64
        } else {
            v.saturating_sub(1)
        };
    }
    Ok(entries)
}

/// Hilbert tile id, as defined by the PMTiles spec (port of the reference
/// implementation; wrapping arithmetic mirrors its two's-complement math).
pub fn zxy_to_tile_id(z: u8, x: u32, y: u32) -> u64 {
    let acc: u64 = ((1u64 << (2 * z as u64)) - 1) / 3;
    let n: u64 = 1u64 << z;
    let (mut tx, mut ty) = (x as u64, y as u64);
    let mut d: u64 = 0;
    let mut s = n / 2;
    while s > 0 {
        let rx: u64 = u64::from(tx & s > 0);
        let ry: u64 = u64::from(ty & s > 0);
        d += s * s * ((3 * rx) ^ ry);
        if ry == 0 {
            if rx == 1 {
                tx = s.wrapping_sub(1).wrapping_sub(tx);
                ty = s.wrapping_sub(1).wrapping_sub(ty);
            }
            std::mem::swap(&mut tx, &mut ty);
        }
        s /= 2;
    }
    acc + d
}

fn find(entries: &[Entry], tile_id: u64) -> Option<Entry> {
    // last entry whose tile_id <= target
    let idx = match entries.binary_search_by(|e| e.tile_id.cmp(&tile_id)) {
        Ok(i) => i,
        Err(0) => return None,
        Err(i) => i - 1,
    };
    let e = entries[idx];
    if e.run_length == 0 {
        return Some(e); // leaf directory pointer
    }
    if tile_id - e.tile_id < e.run_length as u64 {
        return Some(e);
    }
    None
}

impl PmTiles {
    pub fn open(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
        // SAFETY: the file is opened read-only and is not modified while mapped;
        // packs are replaced by writing a new file and renaming it.
        let mmap = unsafe { Mmap::map(&file)? };
        if mmap.len() < 127 || &mmap[0..7] != b"PMTiles" || mmap[7] != 3 {
            bail!("{} is not a PMTiles v3 file", path.display());
        }
        let b = &mmap[..127];
        let header = Header {
            root_offset: u64_at(b, 8),
            root_length: u64_at(b, 16),
            metadata_offset: u64_at(b, 24),
            metadata_length: u64_at(b, 32),
            leaf_offset: u64_at(b, 40),
            data_offset: u64_at(b, 56),
            internal_compression: b[97],
            tile_compression: b[98],
            min_zoom: b[100],
            max_zoom: b[101],
            bounds: [
                i32_at(b, 102) as f64 / 1e7,
                i32_at(b, 106) as f64 / 1e7,
                i32_at(b, 110) as f64 / 1e7,
                i32_at(b, 114) as f64 / 1e7,
            ],
        };
        let root_raw = &mmap[header.root_offset as usize..(header.root_offset + header.root_length) as usize];
        let root = parse_directory(&decompress(root_raw, header.internal_compression)?)?;
        Ok(Self { path: path.to_path_buf(), mmap, header, root, leaves: Mutex::new(HashMap::new()) })
    }

    pub fn metadata(&self) -> Result<serde_json::Value> {
        let h = &self.header;
        let raw = &self.mmap[h.metadata_offset as usize..(h.metadata_offset + h.metadata_length) as usize];
        Ok(serde_json::from_slice(&decompress(raw, h.internal_compression)?)?)
    }

    /// True if the tile at z/x/y could be inside this archive's bounds.
    pub fn may_contain(&self, z: u8, x: u32, y: u32) -> bool {
        if z < self.header.min_zoom || z > self.header.max_zoom {
            return false;
        }
        let [w, s, e, n] = self.header.bounds;
        let (tw, tn) = tile_to_lonlat(z, x, y);
        let (te, ts) = tile_to_lonlat(z, x + 1, y + 1);
        !(te < w || tw > e || tn < s || ts > n)
    }

    fn leaf(&self, offset: u64, length: u32) -> Result<Arc<Vec<Entry>>> {
        if let Some(d) = self.leaves.lock().unwrap().get(&offset) {
            return Ok(d.clone());
        }
        let start = (self.header.leaf_offset + offset) as usize;
        let raw = &self.mmap[start..start + length as usize];
        let dir = Arc::new(parse_directory(&decompress(raw, self.header.internal_compression)?)?);
        let mut cache = self.leaves.lock().unwrap();
        if cache.len() > 512 {
            cache.clear();
        }
        cache.insert(offset, dir.clone());
        Ok(dir)
    }

    /// Returns the decompressed tile bytes, or None if the tile is absent.
    pub fn get(&self, z: u8, x: u32, y: u32) -> Result<Option<Vec<u8>>> {
        let id = zxy_to_tile_id(z, x, y);
        let mut entry = find(&self.root, id);
        for _ in 0..4 {
            match entry {
                None => return Ok(None),
                Some(e) if e.run_length > 0 => {
                    let start = (self.header.data_offset + e.offset) as usize;
                    let raw = &self.mmap[start..start + e.length as usize];
                    return Ok(Some(decompress(raw, self.header.tile_compression)?));
                }
                Some(e) => {
                    let dir = self.leaf(e.offset, e.length)?;
                    entry = find(&dir, id);
                }
            }
        }
        Ok(None)
    }
}

fn tile_to_lonlat(z: u8, x: u32, y: u32) -> (f64, f64) {
    let n = (1u64 << z) as f64;
    let lon = x as f64 / n * 360.0 - 180.0;
    let lat = (std::f64::consts::PI * (1.0 - 2.0 * y as f64 / n)).sinh().atan().to_degrees();
    (lon, lat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_ids_match_spec() {
        // Values from the PMTiles reference implementation.
        assert_eq!(zxy_to_tile_id(0, 0, 0), 0);
        assert_eq!(zxy_to_tile_id(1, 0, 0), 1);
        assert_eq!(zxy_to_tile_id(1, 0, 1), 2);
        assert_eq!(zxy_to_tile_id(1, 1, 1), 3);
        assert_eq!(zxy_to_tile_id(1, 1, 0), 4);
        assert_eq!(zxy_to_tile_id(2, 0, 0), 5);
        assert_eq!(zxy_to_tile_id(4, 11, 5), 307);
        assert_eq!(zxy_to_tile_id(10, 571, 779), 943327);
        assert_eq!(zxy_to_tile_id(14, 12472, 16161), 262878912);
        assert_eq!(zxy_to_tile_id(13, 5574, 3679), 77772478);
        assert_eq!(zxy_to_tile_id(13, 5624, 3313), 77485504);
        assert_eq!(zxy_to_tile_id(3, 3, 0), 26);
        assert_eq!(zxy_to_tile_id(6, 54, 22), 4567);
        assert_eq!(zxy_to_tile_id(4, 7, 11), 197);
    }

    /// Compares against ids generated by the pmtiles JS library, when present.
    #[test]
    fn reference_ids_file() {
        let Ok(raw) = std::fs::read_to_string("/tmp/pmt/ids.json") else { return };
        let ids: Vec<(u8, u32, u32, u64)> = serde_json::from_str(&raw).unwrap();
        for (z, x, y, t) in ids {
            assert_eq!(zxy_to_tile_id(z, x, y), t, "z{z} x{x} y{y}");
        }
    }
}
