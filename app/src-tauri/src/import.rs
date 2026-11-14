//! Installing packs from files the user already has: downloaded from the
//! GitHub release on another computer and carried over on a USB drive, for
//! example.
//!
//! The user picks files or a whole folder (or drops them on the window).
//! Packs are recognised by their release file names:
//!   <id>-map.pmtiles, <id>-search.tar.gz, <id>-routing.tar.gz
//! and big files split for GitHub as <name>.part00, <name>.part01, ...
//! which are joined here. If the folder also holds the release's
//! catalog.json, every file is checked against its SHA-256, and the
//! catalog becomes the app's list of states (useful when the app has never
//! been online).

use crate::packs::{Catalog, CatalogComponent, Progress};
use crate::pmtiles::PmTiles;
use crate::state::AppState;
use anyhow::{anyhow, bail, Context, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const KINDS: [(&str, &str); 3] = [("map", "-map.pmtiles"), ("search", "-search.tar.gz"), ("routing", "-routing.tar.gz")];

const KIND_LABEL: [(&str, &str); 3] = [("map", "map"), ("search", "search"), ("routing", "directions")];

const STATES: [(&str, &str); 51] = [
    ("al", "Alabama"), ("ak", "Alaska"), ("az", "Arizona"), ("ar", "Arkansas"), ("ca", "California"),
    ("co", "Colorado"), ("ct", "Connecticut"), ("de", "Delaware"), ("dc", "District of Columbia"),
    ("fl", "Florida"), ("ga", "Georgia"), ("hi", "Hawaii"), ("id", "Idaho"), ("il", "Illinois"),
    ("in", "Indiana"), ("ia", "Iowa"), ("ks", "Kansas"), ("ky", "Kentucky"), ("la", "Louisiana"),
    ("me", "Maine"), ("md", "Maryland"), ("ma", "Massachusetts"), ("mi", "Michigan"), ("mn", "Minnesota"),
    ("ms", "Mississippi"), ("mo", "Missouri"), ("mt", "Montana"), ("ne", "Nebraska"), ("nv", "Nevada"),
    ("nh", "New Hampshire"), ("nj", "New Jersey"), ("nm", "New Mexico"), ("ny", "New York"),
    ("nc", "North Carolina"), ("nd", "North Dakota"), ("oh", "Ohio"), ("ok", "Oklahoma"), ("or", "Oregon"),
    ("pa", "Pennsylvania"), ("ri", "Rhode Island"), ("sc", "South Carolina"), ("sd", "South Dakota"),
    ("tn", "Tennessee"), ("tx", "Texas"), ("ut", "Utah"), ("vt", "Vermont"), ("va", "Virginia"),
    ("wa", "Washington"), ("wv", "West Virginia"), ("wi", "Wisconsin"), ("wy", "Wyoming"),
];

#[derive(Debug, Default, Serialize)]
pub struct ImportReport {
    /// e.g. "Ohio map"
    pub imported: Vec<String>,
    /// Files that were not imported, with the reason.
    pub skipped: Vec<String>,
    /// Packs that were already installed from the same file.
    pub already: Vec<String>,
    pub warnings: Vec<String>,
    /// True if a catalog.json was found and is now the app's list of states.
    pub catalog_updated: bool,
    /// Region to show on the map afterwards.
    pub first_region: Option<String>,
}

/// One pack found on disk, possibly in several parts.
#[derive(Debug)]
struct Found {
    region: String,
    kind: &'static str,
    /// Release file name, e.g. "ca-search.tar.gz"
    file: String,
    /// The whole file, or its parts in order.
    sources: Vec<PathBuf>,
}

/// Files found for one pack: the whole file and/or numbered parts.
struct Group {
    region: String,
    kind: &'static str,
    whole: Option<PathBuf>,
    parts: BTreeMap<u32, PathBuf>,
}

/// Removes the " (1)" a browser adds when a file name already exists.
fn strip_copy_suffix(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut rest = name;
    while let Some(i) = rest.find(" (") {
        let after = &rest[i + 2..];
        let digits = after.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 && after[digits..].starts_with(')') {
            out.push_str(&rest[..i]);
            rest = &after[digits + 1..];
        } else {
            out.push_str(&rest[..i + 2]);
            rest = after;
        }
    }
    out.push_str(rest);
    out
}

/// What a file name means: (release file, region, kind, part number).
fn classify(name: &str) -> Option<(String, String, &'static str, Option<u32>)> {
    let name = strip_copy_suffix(&name.to_lowercase());
    let (base, part) = match name.rfind(".part") {
        Some(i) if name.len() > i + 5 && name[i + 5..].chars().all(|c| c.is_ascii_digit()) => {
            (&name[..i], Some(name[i + 5..].parse().ok()?))
        }
        _ => (name.as_str(), None),
    };
    for (kind, suffix) in KINDS {
        if let Some(id) = base.strip_suffix(suffix) {
            if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                return Some((base.to_string(), id.to_string(), kind, part));
            }
        }
    }
    None
}

fn is_catalog(name: &str) -> bool {
    let n = strip_copy_suffix(&name.to_lowercase());
    n == "catalog.json"
}

fn walk(path: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    if path.is_dir() {
        if depth == 0 {
            return;
        }
        if let Ok(rd) = std::fs::read_dir(path) {
            let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
            entries.sort();
            for p in entries {
                walk(&p, depth - 1, out);
            }
        }
    } else if path.is_file() {
        out.push(path.to_path_buf());
    }
}

fn label(state: &AppState, catalog: Option<&Catalog>, region: &str) -> String {
    catalog
        .and_then(|c| c.regions.iter().find(|r| r.id == region))
        .map(|r| r.name.clone())
        .or_else(|| state.installed.lock().unwrap().regions.get(region).map(|r| r.name.clone()))
        .or_else(|| STATES.iter().find(|(id, _)| *id == region).map(|(_, n)| n.to_string()))
        .unwrap_or_else(|| region.to_uppercase())
}

fn kind_label(kind: &str) -> &'static str {
    KIND_LABEL.iter().find(|(k, _)| *k == kind).map(|(_, l)| *l).unwrap_or("pack")
}

/// Reads all sources in order, hashing them and (if `copy_to` is set)
/// writing them to one joined file. Reports progress as "verifying".
fn read_through(emit: &dyn Fn(Progress), state: &AppState, f: &Found, total: u64, copy_to: Option<&Path>) -> Result<String> {
    let key = format!("{}:{}", f.region, f.kind);
    let mut h = Sha256::new();
    let mut out = match copy_to {
        Some(p) => Some(std::io::BufWriter::with_capacity(1 << 20, std::fs::File::create(p)?)),
        None => None,
    };
    let mut buf = vec![0u8; 1 << 20];
    let mut done = 0u64;
    let mut last = Instant::now();
    for src in &f.sources {
        let mut file = std::fs::File::open(src).with_context(|| format!("opening {}", src.display()))?;
        loop {
            let n = file.read(&mut buf).with_context(|| format!("reading {}", src.display()))?;
            if n == 0 {
                break;
            }
            if state.cancel.lock().unwrap().remove(&key) {
                bail!("cancelled");
            }
            h.update(&buf[..n]);
            if let Some(o) = out.as_mut() {
                o.write_all(&buf[..n]).context("writing to the data folder (is the disk full?)")?;
            }
            done += n as u64;
            if last.elapsed() > Duration::from_millis(200) {
                last = Instant::now();
                emit(Progress {
                    region: f.region.clone(),
                    component: f.kind.into(),
                    stage: "verifying".into(),
                    received: done,
                    total,
                    message: None,
                });
            }
        }
    }
    if let Some(mut o) = out {
        o.flush()?;
    }
    Ok(hex::encode(h.finalize()))
}

/// `emit` receives progress for the Downloads list.
pub fn import(emit: &dyn Fn(Progress), state: &Arc<AppState>, inputs: &[PathBuf]) -> Result<ImportReport> {
    let mut report = ImportReport::default();
    let mut files = Vec::new();
    for p in inputs {
        walk(p, 3, &mut files);
    }

    // The newest catalog.json among the files, if any.
    let mut catalog: Option<(Catalog, Vec<u8>)> = None;
    let mut groups: BTreeMap<String, Group> = BTreeMap::new();
    for path in &files {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if is_catalog(name) {
            match std::fs::read(path).map_err(anyhow::Error::from).and_then(|b| {
                let c: Catalog = serde_json::from_slice(&b).context("not a ReflectMaps catalog")?;
                Ok((c, b))
            }) {
                Ok((c, b)) => {
                    if catalog.as_ref().is_none_or(|(old, _)| (c.built.as_str(), c.tag.as_str()) > (old.built.as_str(), old.tag.as_str())) {
                        catalog = Some((c, b));
                    }
                }
                Err(e) => report.skipped.push(format!("{name}: {e:#}")),
            }
            continue;
        }
        let Some((file, region, kind, part)) = classify(name) else {
            if !name.ends_with(".meta.json") && !name.starts_with('.') {
                report.skipped.push(format!("{name}: not a ReflectMaps pack"));
            }
            continue;
        };
        let g = groups.entry(file).or_insert_with(|| Group { region, kind, whole: None, parts: BTreeMap::new() });
        match part {
            None => g.whole = Some(path.clone()),
            Some(n) => {
                g.parts.insert(n, path.clone());
            }
        }
    }

    if let Some((c, bytes)) = &catalog {
        // Make it the app's list of states unless the app already has a newer one.
        let cached_bytes = std::fs::read(state.paths.catalog_cache()).ok();
        let cached: Option<Catalog> = cached_bytes.as_ref().and_then(|b| serde_json::from_slice(b).ok());
        let newer = cached.is_none_or(|old| (c.built.as_str(), c.tag.as_str()) >= (old.built.as_str(), old.tag.as_str()));
        if newer && cached_bytes.as_ref() != Some(bytes) {
            std::fs::write(state.paths.catalog_cache(), bytes)?;
            report.catalog_updated = true;
        }
    }
    let cat = catalog.as_ref().map(|(c, _)| c);

    if groups.is_empty() {
        if report.catalog_updated {
            return Ok(report);
        }
        bail!(
            "No map packs found. Pack files are named like oh-map.pmtiles, oh-search.tar.gz and oh-routing.tar.gz."
        );
    }

    let mut found = Vec::new();
    for (file, Group { region, kind, whole, parts }) in groups {
        let expected = cat
            .and_then(|c| c.regions.iter().find(|r| r.id == region))
            .and_then(|r| r.components.get(kind))
            .filter(|c| c.file == file);
        let sources = if let Some(w) = whole {
            vec![w]
        } else {
            // Parts must run 00, 01, 02 ... with none missing.
            let want = expected.map(|c| c.parts.len()).filter(|n| *n > 0);
            let missing: Vec<String> = (0..want.unwrap_or(parts.keys().max().map_or(0, |m| m + 1) as usize) as u32)
                .filter(|n| !parts.contains_key(n))
                .map(|n| format!("{file}.part{n:02}"))
                .collect();
            if !missing.is_empty() {
                report.skipped.push(format!(
                    "{} {}: missing {}",
                    label(state, cat, &region),
                    kind_label(kind),
                    missing.join(", ")
                ));
                continue;
            }
            parts.into_values().collect()
        };
        found.push(Found { region, kind, file, sources });
    }

    let mut any_unchecked = false;
    let mut routing_changed = false;
    for f in &found {
        let name = label(state, cat, &f.region);
        let what = format!("{name} {}", kind_label(f.kind));
        {
            let mut busy = state.busy.lock().unwrap();
            if !busy.insert(f.region.clone()) {
                report.skipped.push(format!("{what}: {name} is downloading right now"));
                continue;
            }
        }
        let res = import_one(emit, state, cat, f, &name);
        state.busy.lock().unwrap().remove(&f.region);
        match res {
            Ok(Outcome { checked, already }) => {
                emit(Progress {
                    region: f.region.clone(),
                    component: f.kind.into(),
                    stage: "done".into(),
                    received: 0,
                    total: 0,
                    message: None,
                });
                if already {
                    report.already.push(what);
                } else {
                    report.imported.push(what);
                    report.first_region.get_or_insert_with(|| f.region.clone());
                    routing_changed |= f.kind == "routing";
                }
                any_unchecked |= !checked;
            }
            Err(e) if e.to_string() == "cancelled" => {
                emit(Progress {
                    region: f.region.clone(),
                    component: f.kind.into(),
                    stage: "cancelled".into(),
                    received: 0,
                    total: 0,
                    message: None,
                });
                report.skipped.push(format!("{what}: stopped"));
            }
            Err(e) => {
                emit(Progress {
                    region: f.region.clone(),
                    component: f.kind.into(),
                    stage: "error".into(),
                    received: 0,
                    total: 0,
                    message: Some(format!("Import failed: {e:#}")),
                });
                report.skipped.push(format!("{what}: {e:#}"));
            }
        }
    }

    if routing_changed {
        crate::routing::refresh(state);
        // Routing packs only join up at state lines when they come from the
        // same build of the road network.
        let inst = state.installed.lock().unwrap();
        let mut builds: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for r in inst.regions.values() {
            if let Some(c) = r.components.get("routing") {
                builds.entry(c.graph_build.clone()).or_default().push(r.name.clone());
            }
        }
        if builds.len() > 1 {
            let list: Vec<String> = builds.iter().map(|(b, n)| format!("{} ({})", n.join(", "), if b.is_empty() { "unknown build" } else { b })).collect();
            report.warnings.push(format!(
                "Directions packs come from different data builds: {}. Routes that cross between them may fail; import directions packs from one release.",
                list.join("; ")
            ));
        }
    }
    if any_unchecked {
        report.warnings.push(if cat.is_some() {
            "Some files are not from the same release as catalog.json, so they could not be checked for damage.".into()
        } else {
            "No catalog.json was included, so the files could not be checked for damage. Copy catalog.json from the same GitHub release next time.".into()
        });
    }
    Ok(report)
}

struct Outcome {
    /// The checksum matched the catalog.
    checked: bool,
    /// The same file was already installed.
    already: bool,
}

fn import_one(emit: &dyn Fn(Progress), state: &Arc<AppState>, cat: Option<&Catalog>, f: &Found, name: &str) -> Result<Outcome> {
    let total: u64 = f.sources.iter().map(|p| p.metadata().map(|m| m.len()).unwrap_or(0)).sum();
    let expected: Option<&CatalogComponent> = cat
        .and_then(|c| c.regions.iter().find(|r| r.id == f.region))
        .and_then(|r| r.components.get(f.kind))
        .filter(|c| c.file == f.file);
    if let Some(e) = expected {
        // Parts listed in catalog.json must add up to the whole file.
        if f.sources.len() > 1 && f.sources.len() == e.parts.len() && total != e.size {
            bail!("the parts do not add up to the full file; one may be incomplete, copy them again");
        }
    }

    // A map is copied (joined if in parts) into the data folder. An archive
    // in one piece is unpacked straight from where it is; in parts, it is
    // joined first.
    let needs_copy = f.kind == "map" || f.sources.len() > 1;
    let tmp = state.paths.downloads().join(format!("{}.import", f.file));
    let sha = read_through(emit, state, f, total, needs_copy.then_some(tmp.as_path())).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })?;
    let cleanup = || {
        if needs_copy {
            let _ = std::fs::remove_file(&tmp);
        }
    };

    if state
        .installed
        .lock()
        .unwrap()
        .regions
        .get(&f.region)
        .and_then(|r| r.components.get(f.kind))
        .is_some_and(|c| c.sha256 == sha)
    {
        cleanup();
        return Ok(Outcome { checked: true, already: true });
    }

    let mut checked = false;
    if let Some(e) = expected {
        if e.sha256 == sha {
            checked = true;
        } else if e.size == total {
            cleanup();
            bail!("the file is damaged (checksum does not match catalog.json); copy it from GitHub again");
        }
    }
    let tag = match (checked, cat) {
        (true, Some(c)) => c.tag.clone(),
        _ => "imported".to_string(),
    };
    let bbox = cat
        .and_then(|c| c.regions.iter().find(|r| r.id == f.region))
        .and_then(|r| r.bbox)
        .or_else(|| {
            if f.kind == "map" {
                PmTiles::open(&tmp).ok().map(|p| p.header.bounds)
            } else {
                None
            }
        });

    emit(Progress {
        region: f.region.clone(),
        component: f.kind.into(),
        stage: "installing".into(),
        received: total,
        total,
        message: None,
    });
    let src = if needs_copy { tmp.clone() } else { f.sources[0].clone() };
    let res = crate::packs::install_file(state, &f.region, name, bbox, f.kind, &src, &tag, (&f.file, total, &sha));
    cleanup();
    res.map_err(|e| anyhow!("{e:#} (the file may be incomplete or damaged; copy it again)"))?;
    Ok(Outcome { checked, already: false })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(classify("oh-map.pmtiles").map(|c| (c.1, c.2, c.3)), Some(("oh".into(), "map", None)));
        assert_eq!(classify("CA-search.tar.gz.part01").map(|c| (c.0, c.2, c.3)), Some(("ca-search.tar.gz".into(), "search", Some(1))));
        assert_eq!(classify("ky-routing (2).tar.gz").map(|c| c.1), Some("ky".into()));
        assert_eq!(classify("ky-routing.tar.gz (1).part00").map(|c| c.3), Some(Some(0)));
        assert!(classify("oh-map.pmtiles.meta.json").is_none());
        assert!(classify("-map.pmtiles").is_none());
        assert!(classify("notes.txt").is_none());
        assert!(is_catalog("catalog (3).json"));
        assert_eq!(strip_copy_suffix("a (x) b (12).c"), "a (x) b.c");
    }
}

/// End-to-end check against real pack files:
/// RM_IMPORT_TEST_SRC=<folder of packs> RM_IMPORT_TEST_DATA=<empty data dir> \
///   cargo test -p reflectmaps --lib import::e2e -- --ignored --nocapture
#[cfg(test)]
mod e2e {
    use super::*;
    use crate::state::Paths;

    #[test]
    #[ignore]
    fn import_folder() {
        let src = PathBuf::from(std::env::var("RM_IMPORT_TEST_SRC").unwrap());
        let data = PathBuf::from(std::env::var("RM_IMPORT_TEST_DATA").unwrap());
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        let state = Arc::new(AppState::new(Paths { data, resources }));
        state.load();
        crate::routing::refresh(&state);
        let last = std::sync::Mutex::new(String::new());
        let emit = |p: Progress| {
            let line = format!("{} {} {}", p.region, p.component, p.stage);
            let mut l = last.lock().unwrap();
            if *l != line {
                eprintln!("  progress: {line} {}/{}", p.received, p.total);
                *l = line;
            }
        };
        let report = import(&emit, &state, &[src]).unwrap();
        eprintln!("{}", serde_json::to_string_pretty(&report).unwrap());
        let hits = state.search.read().unwrap().search("white house", rm_search::QueryOptions { near: None, limit: 3 }).unwrap();
        for h in hits {
            eprintln!("search: {} | {}", h.name, h.label);
        }
        eprintln!("routing ready: {}", state.routing.lock().unwrap().is_some());
        if let Ok(q) = std::env::var("RM_IMPORT_TEST_ROUTE") {
            let req: crate::routing::RouteRequest = serde_json::from_str(&q).unwrap();
            let g = state.routing.lock().unwrap();
            match crate::routing::plan(g.as_ref().unwrap(), &req) {
                Ok(opts) => opts.iter().for_each(|o| eprintln!("route: {} {:.1} mi {} min", o.label, o.distance_mi, o.time_s / 60.0)),
                Err(e) => eprintln!("route error: {e:#}"),
            }
        }
        eprintln!("installed: {}", serde_json::to_string(&state.installed.lock().unwrap().regions.iter().map(|(k, r)| (k.clone(), r.name.clone(), r.bbox, r.components.iter().map(|(c, v)| format!("{c}:{}", v.tag)).collect::<Vec<_>>())).collect::<Vec<_>>()).unwrap());
    }
}
