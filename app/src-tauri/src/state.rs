//! App-wide state: where data lives, what is installed, open readers.

use crate::pmtiles::PmTiles;
use anyhow::Result;
use rm_search::Searcher;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstalledComponent {
    /// Data build the file came from (catalog tag), or "imported".
    pub tag: String,
    pub file: String,
    pub size: u64,
    pub sha256: String,
    /// Where it lives inside the data folder (file or directory).
    pub path: String,
    #[serde(default)]
    pub graph_build: String,
    #[serde(default)]
    pub tiles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstalledRegion {
    pub name: String,
    #[serde(default)]
    pub bbox: Option<[f64; 4]>,
    pub components: BTreeMap<String, InstalledComponent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Installed {
    pub regions: BTreeMap<String, InstalledRegion>,
}

#[derive(Clone)]
pub struct Paths {
    pub data: PathBuf,
    pub resources: PathBuf,
}

impl Paths {
    pub fn maps(&self) -> PathBuf {
        self.data.join("maps")
    }
    pub fn search(&self) -> PathBuf {
        self.data.join("search")
    }
    pub fn routing(&self) -> PathBuf {
        self.data.join("routing")
    }
    pub fn routing_tiles(&self) -> PathBuf {
        self.routing().join("tiles")
    }
    pub fn downloads(&self) -> PathBuf {
        self.data.join("downloads")
    }
    pub fn installed_file(&self) -> PathBuf {
        self.data.join("installed.json")
    }
    pub fn catalog_cache(&self) -> PathBuf {
        self.data.join("catalog.json")
    }
    pub fn queue_file(&self) -> PathBuf {
        self.data.join("queue.json")
    }
    pub fn manifests(&self) -> PathBuf {
        self.data.join("manifests")
    }
    pub fn ensure(&self) -> Result<()> {
        for d in [self.maps(), self.search(), self.routing_tiles(), self.downloads()] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }
}

pub struct AppState {
    pub paths: Paths,
    /// Map tile archives, most detailed first; the world overview is last.
    pub tiles: RwLock<Vec<(String, Arc<PmTiles>)>>,
    pub search: RwLock<Searcher>,
    pub installed: Mutex<Installed>,
    pub cancel: Mutex<HashSet<String>>,
    pub busy: Mutex<HashSet<String>>,
    /// Newest search number per search box, to cancel older searches.
    pub search_seq: Mutex<std::collections::HashMap<String, u64>>,
    pub routing: Mutex<Option<crate::routing::Valhalla>>,
    pub routing_error: Mutex<Option<String>>,
    /// Running from an external drive with its data next to the program.
    pub portable: bool,
    pub queue: crate::queue::Queue,
}

impl AppState {
    pub fn new(paths: Paths) -> Self {
        Self {
            paths,
            tiles: RwLock::new(Vec::new()),
            search: RwLock::new(Searcher::new()),
            installed: Mutex::new(Installed::default()),
            cancel: Mutex::new(HashSet::new()),
            busy: Mutex::new(HashSet::new()),
            routing: Mutex::new(None),
            routing_error: Mutex::new(None),
            portable: false,
            search_seq: Default::default(),
            queue: Default::default(),
        }
    }

    pub fn save_installed(&self) -> Result<()> {
        let inst = self.installed.lock().unwrap().clone();
        let tmp = self.paths.installed_file().with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&inst)?)?;
        std::fs::rename(tmp, self.paths.installed_file())?;
        Ok(())
    }

    /// Opens everything recorded in installed.json plus the bundled base data.
    pub fn load(&self) {
        let _ = self.paths.ensure();
        let inst: Installed = std::fs::read(self.paths.installed_file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let mut tiles = Vec::new();
        let mut search = Searcher::new();
        for (id, r) in &inst.regions {
            if let Some(c) = r.components.get("map") {
                match PmTiles::open(&self.paths.data.join(&c.path)) {
                    Ok(p) => tiles.push((id.clone(), Arc::new(p))),
                    Err(e) => eprintln!("map pack {id}: {e:#}"),
                }
            }
            if let Some(c) = r.components.get("search") {
                if let Err(e) = search.add_index(&format!("{id}-search"), &self.paths.data.join(&c.path)) {
                    eprintln!("search pack {id}: {e:#}");
                }
            }
        }
        let world = self.paths.resources.join("world-base.pmtiles");
        if world.exists() {
            match PmTiles::open(&world) {
                Ok(p) => tiles.push(("world".into(), Arc::new(p))),
                Err(e) => eprintln!("world overview: {e:#}"),
            }
        }
        // The search library needs a writable folder (it takes a lock file),
        // and installed apps often live in read-only locations, so the
        // bundled city list is copied to the data folder first.
        let bundled = self.paths.resources.join("us-areas");
        let areas = self.paths.data.join("base").join("us-areas");
        if bundled.join("meta.json").exists()
            && std::fs::read(bundled.join("meta.json")).ok() != std::fs::read(areas.join("meta.json")).ok()
        {
            let tmp = areas.with_extension("new");
            let _ = std::fs::remove_dir_all(&tmp);
            match copy_index(&bundled, &tmp) {
                Ok(()) => {
                    let _ = std::fs::remove_dir_all(&areas);
                    if let Err(e) = std::fs::rename(&tmp, &areas) {
                        eprintln!("areas index: {e:#}");
                    }
                }
                Err(e) => eprintln!("copying areas index: {e:#}"),
            }
        }
        if areas.join("meta.json").exists() {
            if let Err(e) = search.add_index("us-areas", &areas) {
                eprintln!("areas index: {e:#}");
            }
        }
        cleanup_unreferenced(&self.paths, &inst);
        *self.tiles.write().unwrap() = tiles;
        *self.search.write().unwrap() = search;
        *self.installed.lock().unwrap() = inst;
    }
}

fn copy_index(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let name = e.file_name();
        if name.to_string_lossy().ends_with(".lock") {
            continue;
        }
        if e.file_type()?.is_dir() {
            copy_index(&e.path(), &dst.join(&name))?;
        } else {
            std::fs::copy(e.path(), dst.join(&name))?;
        }
    }
    Ok(())
}

/// Deletes replaced pack files that could not be removed while in use.
fn cleanup_unreferenced(paths: &Paths, inst: &Installed) {
    let keep: HashSet<PathBuf> = inst
        .regions
        .values()
        .flat_map(|r| r.components.values())
        .map(|c| paths.data.join(&c.path))
        .collect();
    for dir in [paths.maps(), paths.search()] {
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let p = e.path();
                if !keep.contains(&p) {
                    let _ = if p.is_dir() { std::fs::remove_dir_all(&p) } else { std::fs::remove_file(&p) };
                }
            }
        }
    }
}

pub fn rel(base: &Path, p: &Path) -> String {
    p.strip_prefix(base).unwrap_or(p).to_string_lossy().replace('\\', "/")
}
