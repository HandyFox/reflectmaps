//! "Check data": confirms every installed file is intact, e.g. on a drive
//! that was copied from another one.
//!
//! A map pack is checked against the checksum it was downloaded with. Search
//! and directions packs are unpacked into many files, so when one is
//! installed a manifest of its files and their SHA-256 is written to
//! `manifests/`; checking compares against that.

use crate::state::AppState;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Manifest {
    /// (path inside the data folder, size, sha256)
    pub files: Vec<(String, u64, String)>,
}

fn hash_file(path: &Path, mut on_bytes: impl FnMut(u64)) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        on_bytes(n as u64);
    }
    Ok(hex::encode(h.finalize()))
}

fn rel(data: &Path, p: &Path) -> String {
    p.strip_prefix(data).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.file_name().is_some_and(|n| n != ".tantivy-meta.lock" && n != ".tantivy-writer.lock") {
                out.push(p);
            }
        }
    }
}

/// Records the files of a freshly installed search or directions pack.
/// `files` are paths inside the data folder; a directory means all files in it.
pub fn write_manifest(state: &AppState, region: &str, kind: &str, files: &[std::path::PathBuf]) -> Result<()> {
    let data = &state.paths.data;
    let mut all = Vec::new();
    for f in files {
        if f.is_dir() {
            walk(f, &mut all);
        } else {
            all.push(f.clone());
        }
    }
    all.sort();
    let mut m = Manifest::default();
    for p in all {
        let size = p.metadata()?.len();
        m.files.push((rel(data, &p), size, hash_file(&p, |_| {})?));
    }
    let dir = state.paths.manifests();
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(format!("{region}-{kind}.json")), serde_json::to_vec(&m)?)?;
    Ok(())
}

pub fn remove_manifest(state: &AppState, region: &str, kind: &str) {
    let _ = std::fs::remove_file(state.paths.manifests().join(format!("{region}-{kind}.json")));
}

#[derive(Debug, Clone, Serialize)]
pub struct CheckProgress {
    pub done: u64,
    pub total: u64,
    pub region: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RegionCheck {
    pub region: String,
    pub name: String,
    /// "ok", "damaged" or "unchecked"
    pub status: String,
    pub problems: Vec<String>,
}

const LABEL: [(&str, &str); 3] = [("map", "Map"), ("search", "Search"), ("routing", "Directions")];

pub fn check_all(state: &AppState, progress: &dyn Fn(CheckProgress), stop: &dyn Fn() -> bool) -> Result<Vec<RegionCheck>> {
    let data = state.paths.data.clone();
    let inst = state.installed.lock().unwrap().clone();

    // What to read, per region and part, and how many bytes in all.
    struct Job {
        region: String,
        kind: String,
        files: Vec<(String, u64, String)>,
        manifest: bool,
    }
    let mut jobs = Vec::new();
    for (id, r) in &inst.regions {
        for (kind, c) in &r.components {
            if kind == "map" {
                jobs.push(Job { region: id.clone(), kind: kind.clone(), files: vec![(c.path.clone(), c.size, c.sha256.clone())], manifest: true });
            } else {
                let mpath = state.paths.manifests().join(format!("{id}-{kind}.json"));
                match std::fs::read(&mpath).ok().and_then(|b| serde_json::from_slice::<Manifest>(&b).ok()) {
                    Some(m) => jobs.push(Job { region: id.clone(), kind: kind.clone(), files: m.files, manifest: true }),
                    None => jobs.push(Job { region: id.clone(), kind: kind.clone(), files: Vec::new(), manifest: false }),
                }
            }
        }
    }
    let total: u64 = jobs.iter().flat_map(|j| j.files.iter().map(|f| f.1)).sum();
    let mut done = 0u64;
    let mut last = Instant::now();

    let mut out: Vec<RegionCheck> = inst
        .regions
        .iter()
        .map(|(id, r)| RegionCheck { region: id.clone(), name: r.name.clone(), status: "ok".into(), problems: Vec::new() })
        .collect();

    for job in jobs {
        let rc = out.iter_mut().find(|r| r.region == job.region).unwrap();
        let part = LABEL.iter().find(|(k, _)| *k == job.kind).map(|(_, l)| *l).unwrap_or("Pack");
        if !job.manifest {
            // Installed by a version before 0.5: only check it is there.
            let c = &inst.regions[&job.region].components[&job.kind];
            if !data.join(&c.path).exists() {
                rc.status = "damaged".into();
                rc.problems.push(format!("{part}: missing"));
            } else if rc.status == "ok" {
                rc.status = "unchecked".into();
                rc.problems.push(format!("{part}: installed by an older version, so it can't be fully checked"));
            }
            continue;
        }
        let mut bad = 0usize;
        let mut first_bad = None;
        for (path, size, sha) in &job.files {
            if stop() {
                anyhow::bail!("cancelled");
            }
            let full = data.join(path);
            let ok = match full.metadata() {
                Ok(m) if m.len() == *size => {
                    let start = done;
                    let h = hash_file(&full, |n| {
                        done += n;
                        if last.elapsed() > Duration::from_millis(250) {
                            last = Instant::now();
                            progress(CheckProgress { done, total, region: job.region.clone() });
                        }
                    });
                    done = start + size;
                    h.map(|h| sha.is_empty() || h == *sha).unwrap_or(false)
                }
                _ => {
                    done += size;
                    false
                }
            };
            if !ok {
                bad += 1;
                first_bad.get_or_insert_with(|| path.clone());
            }
        }
        if bad > 0 {
            rc.status = "damaged".into();
            rc.problems.push(format!(
                "{part}: {bad} file{} missing or changed (e.g. {})",
                if bad == 1 { "" } else { "s" },
                first_bad.unwrap_or_default()
            ));
        }
    }
    progress(CheckProgress { done: total, total, region: String::new() });
    Ok(out)
}
