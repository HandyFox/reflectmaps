//! Catalog, downloads and installation of map packs.

use crate::pmtiles::PmTiles;
use crate::state::{rel, AppState, InstalledComponent, InstalledRegion};
use anyhow::{anyhow, bail, Context, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncWriteExt;

pub const DEFAULT_CATALOG_URL: &str =
    "https://github.com/HandyFox/reflectmaps/releases/download/data-current/catalog.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogComponent {
    pub file: String,
    pub size: u64,
    pub sha256: String,
    pub url: String,
    #[serde(default)]
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogRegion {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub bbox: Option<[f64; 4]>,
    pub components: BTreeMap<String, CatalogComponent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub schema: u32,
    pub tag: String,
    pub built: String,
    #[serde(default)]
    pub sources: serde_json::Value,
    #[serde(default)]
    pub routing_graph: String,
    #[serde(default)]
    pub base: BTreeMap<String, CatalogComponent>,
    pub regions: Vec<CatalogRegion>,
    #[serde(default)]
    pub attribution: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct CatalogResult {
    pub catalog: Option<Catalog>,
    /// True if the catalog was fetched just now; false if it came from the cache.
    pub online: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub region: String,
    pub component: String,
    /// downloading | verifying | installing | done | error | cancelled
    pub stage: String,
    pub received: u64,
    pub total: u64,
    pub message: Option<String>,
}

fn catalog_url() -> String {
    std::env::var("RM_CATALOG_URL").unwrap_or_else(|_| DEFAULT_CATALOG_URL.to_string())
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(concat!("ReflectMaps/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .build()?)
}

pub async fn get_catalog(state: &AppState, refresh: bool) -> CatalogResult {
    let cached = || -> Option<Catalog> {
        std::fs::read(state.paths.catalog_cache()).ok().and_then(|b| serde_json::from_slice(&b).ok())
    };
    if !refresh {
        if let Some(c) = cached() {
            return CatalogResult { catalog: Some(c), online: false, error: None };
        }
    }
    let fetched: Result<Catalog> = async {
        let resp = client()?.get(catalog_url()).send().await?.error_for_status()?;
        let bytes = resp.bytes().await?;
        let cat: Catalog = serde_json::from_slice(&bytes).context("catalog is not valid")?;
        std::fs::write(state.paths.catalog_cache(), &bytes)?;
        Ok(cat)
    }
    .await;
    match fetched {
        Ok(c) => CatalogResult { catalog: Some(c), online: true, error: None },
        Err(e) => CatalogResult {
            catalog: cached(),
            online: false,
            error: Some(format!("Could not reach the download server: {e}")),
        },
    }
}

fn emit(app: &AppHandle, p: Progress) {
    let _ = app.emit("pack-progress", p);
}

fn sha256_file(path: &Path) -> Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// Downloads one file with resume support. Returns the finished file path.
async fn download(
    app: &AppHandle,
    state: &AppState,
    region: &str,
    kind: &str,
    c: &CatalogComponent,
) -> Result<PathBuf> {
    let part = state.paths.downloads().join(format!("{}.part", c.file));
    let key = format!("{region}:{kind}");
    let mut have = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    if have > c.size {
        std::fs::remove_file(&part)?;
        have = 0;
    }
    if have < c.size {
        let mut req = client()?.get(&c.url);
        if have > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={have}-"));
        }
        let resp = req.send().await?.error_for_status()?;
        let resumed = resp.status() == reqwest::StatusCode::PARTIAL_CONTENT;
        if !resumed {
            have = 0;
        }
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(resumed)
            .truncate(!resumed)
            .open(&part)
            .await?;
        let mut stream = resp.bytes_stream();
        let mut last = Instant::now();
        while let Some(chunk) = stream.next().await {
            if state.cancel.lock().unwrap().remove(&key) {
                file.flush().await?;
                bail!("cancelled");
            }
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            have += chunk.len() as u64;
            if last.elapsed() > Duration::from_millis(200) {
                last = Instant::now();
                emit(app, Progress {
                    region: region.into(),
                    component: kind.into(),
                    stage: "downloading".into(),
                    received: have,
                    total: c.size,
                    message: None,
                });
            }
        }
        file.flush().await?;
    }
    emit(app, Progress {
        region: region.into(),
        component: kind.into(),
        stage: "verifying".into(),
        received: c.size,
        total: c.size,
        message: None,
    });
    let p2 = part.clone();
    let sha = tokio::task::spawn_blocking(move || sha256_file(&p2)).await??;
    if !c.sha256.is_empty() && sha != c.sha256 {
        let _ = std::fs::remove_file(&part);
        bail!("the downloaded file was damaged (checksum mismatch); please try again");
    }
    Ok(part)
}

fn unpack_tar_gz(file: &Path, dest: &Path) -> Result<()> {
    let f = std::fs::File::open(file)?;
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(std::io::BufReader::new(f)));
    ar.set_overwrite(true);
    ar.unpack(dest)?;
    Ok(())
}

/// Installs a verified download. Runs on a blocking thread.
pub fn install_file(
    state: &AppState,
    region_id: &str,
    region_name: &str,
    bbox: Option<[f64; 4]>,
    kind: &str,
    file: &Path,
    tag: &str,
    meta: (&str, u64, &str),
) -> Result<()> {
    let (file_name, size, sha256) = meta;
    let paths = &state.paths;
    let short = tag.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect::<String>();
    let mut comp = InstalledComponent {
        tag: tag.to_string(),
        file: file_name.to_string(),
        size,
        sha256: sha256.to_string(),
        ..Default::default()
    };
    match kind {
        "map" => {
            let dest = paths.maps().join(format!("{region_id}-map-{short}.pmtiles"));
            std::fs::rename(file, &dest).or_else(|_| std::fs::copy(file, &dest).map(|_| ()))?;
            let reader = Arc::new(PmTiles::open(&dest)?);
            let mut tiles = state.tiles.write().unwrap();
            tiles.retain(|(id, _)| id != region_id);
            // regions before the world overview
            let pos = tiles.iter().position(|(id, _)| id == "world").unwrap_or(tiles.len());
            tiles.insert(pos, (region_id.to_string(), reader));
            comp.path = rel(&paths.data, &dest);
        }
        "search" => {
            let tmp = paths.search().join(format!(".unpack-{region_id}"));
            let _ = std::fs::remove_dir_all(&tmp);
            unpack_tar_gz(file, &tmp)?;
            let inner = std::fs::read_dir(&tmp)?
                .flatten()
                .map(|e| e.path())
                .find(|p| p.join("meta.json").exists())
                .ok_or_else(|| anyhow!("search pack has no index"))?;
            let dest = paths.search().join(format!("{region_id}-search-{short}"));
            let _ = std::fs::remove_dir_all(&dest);
            std::fs::rename(&inner, &dest)?;
            let _ = std::fs::remove_dir_all(&tmp);
            state.search.write().unwrap().add_index(&format!("{region_id}-search"), &dest)?;
            comp.path = rel(&paths.data, &dest);
            let _ = std::fs::remove_file(file);
        }
        "routing" => {
            let tmp = paths.routing().join(format!(".unpack-{region_id}"));
            let _ = std::fs::remove_dir_all(&tmp);
            unpack_tar_gz(file, &tmp)?;
            let manifest: serde_json::Value = serde_json::from_slice(&std::fs::read(tmp.join("routing.json"))?)?;
            let tiles: Vec<String> = manifest["tiles"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            for t in &tiles {
                let src = tmp.join("tiles").join(t);
                let dst = paths.routing_tiles().join(t);
                std::fs::create_dir_all(dst.parent().unwrap())?;
                std::fs::rename(&src, &dst).or_else(|_| std::fs::copy(&src, &dst).map(|_| ()))?;
            }
            let _ = std::fs::remove_dir_all(&tmp);
            comp.graph_build = manifest["graph_build"].as_str().unwrap_or_default().to_string();
            comp.tiles = tiles;
            comp.path = rel(&paths.data, &paths.routing_tiles());
            let _ = std::fs::remove_file(file);
        }
        other => bail!("unknown pack component {other}"),
    }
    let old_path = {
        let mut inst = state.installed.lock().unwrap();
        let r = inst.regions.entry(region_id.to_string()).or_insert_with(|| InstalledRegion {
            name: region_name.to_string(),
            bbox,
            components: BTreeMap::new(),
        });
        r.name = region_name.to_string();
        if bbox.is_some() {
            r.bbox = bbox;
        }
        let old = r.components.insert(kind.to_string(), comp.clone()).map(|c| c.path);
        old.filter(|p| *p != comp.path && kind != "routing")
    };
    state.save_installed()?;
    if let Some(p) = old_path {
        let full = paths.data.join(p);
        // May fail on Windows while the old file is still mapped; startup cleans up.
        let _ = if full.is_dir() { std::fs::remove_dir_all(&full) } else { std::fs::remove_file(&full) };
    }
    Ok(())
}

pub async fn install(app: AppHandle, region_id: String, components: Vec<String>) -> Result<()> {
    let state = app.state::<Arc<AppState>>().inner().clone();
    {
        let mut busy = state.busy.lock().unwrap();
        if !busy.insert(region_id.clone()) {
            bail!("already downloading");
        }
    }
    let result = install_inner(&app, &state, &region_id, &components).await;
    state.busy.lock().unwrap().remove(&region_id);
    result
}

async fn install_inner(app: &AppHandle, state: &Arc<AppState>, region_id: &str, components: &[String]) -> Result<()> {
    let cat = get_catalog(state, false).await.catalog.ok_or_else(|| anyhow!("no catalog; connect to the internet once"))?;
    let region = cat
        .regions
        .iter()
        .find(|r| r.id == region_id)
        .cloned()
        .ok_or_else(|| anyhow!("{region_id} is not in the catalog"))?;
    // Routing packs only fit together when they come from the same graph
    // build, so a routing update brings the other installed routing packs along.
    let mut work: Vec<(CatalogRegion, String)> = components.iter().map(|k| (region.clone(), k.clone())).collect();
    if components.iter().any(|k| k == "routing") {
        let inst = state.installed.lock().unwrap().clone();
        for (id, r) in &inst.regions {
            if id == region_id {
                continue;
            }
            if let Some(c) = r.components.get("routing") {
                if c.graph_build != cat.routing_graph {
                    if let Some(other) = cat.regions.iter().find(|x| &x.id == id) {
                        work.push((other.clone(), "routing".into()));
                    }
                }
            }
        }
    }
    for (reg, kind) in work {
        let Some(c) = reg.components.get(&kind).cloned() else { continue };
        let res: Result<()> = async {
            let file = download(app, state, &reg.id, &kind, &c).await?;
            emit(app, Progress {
                region: reg.id.clone(),
                component: kind.clone(),
                stage: "installing".into(),
                received: c.size,
                total: c.size,
                message: None,
            });
            let st = state.clone();
            let (rid, rname, bbox, k, tag) = (reg.id.clone(), reg.name.clone(), reg.bbox, kind.clone(), cat.tag.clone());
            let (fname, size, sha) = (c.file.clone(), c.size, c.sha256.clone());
            tokio::task::spawn_blocking(move || {
                install_file(&st, &rid, &rname, bbox, &k, &file, &tag, (&fname, size, &sha))
            })
            .await??;
            if kind == "routing" {
                crate::routing::refresh(&state.clone());
            }
            Ok(())
        }
        .await;
        match res {
            Ok(()) => emit(app, Progress {
                region: reg.id.clone(),
                component: kind.clone(),
                stage: "done".into(),
                received: c.size,
                total: c.size,
                message: None,
            }),
            Err(e) => {
                let cancelled = e.to_string() == "cancelled";
                emit(app, Progress {
                    region: reg.id.clone(),
                    component: kind.clone(),
                    stage: if cancelled { "cancelled" } else { "error" }.into(),
                    received: 0,
                    total: c.size,
                    message: Some(format!("{e:#}")),
                });
                return Err(e);
            }
        }
    }
    Ok(())
}

pub fn remove(state: &AppState, region_id: &str) -> Result<()> {
    let removed = state.installed.lock().unwrap().regions.remove(region_id);
    let Some(r) = removed else { return Ok(()) };
    state.tiles.write().unwrap().retain(|(id, _)| id != region_id);
    state.search.write().unwrap().remove_index(&format!("{region_id}-search"));
    let still_used: std::collections::HashSet<String> = state
        .installed
        .lock()
        .unwrap()
        .regions
        .values()
        .filter_map(|r| r.components.get("routing"))
        .flat_map(|c| c.tiles.clone())
        .collect();
    for (kind, c) in &r.components {
        if kind == "routing" {
            for t in &c.tiles {
                if !still_used.contains(t) {
                    let _ = std::fs::remove_file(state.paths.routing_tiles().join(t));
                }
            }
        } else {
            let full = state.paths.data.join(&c.path);
            let _ = if full.is_dir() { std::fs::remove_dir_all(&full) } else { std::fs::remove_file(&full) };
        }
    }
    state.save_installed()?;
    crate::routing::refresh(state);
    Ok(())
}

/// Installs a pack file the user already has on disk (e.g. copied on a USB
/// drive). The region and component come from the file name:
/// <id>-map.pmtiles, <id>-search.tar.gz, <id>-routing.tar.gz.
pub fn import(state: &AppState, path: &Path) -> Result<String> {
    let name = path.file_name().and_then(|n| n.to_str()).context("bad file name")?.to_string();
    let (id, kind) = if let Some(id) = name.strip_suffix("-map.pmtiles") {
        (id, "map")
    } else if let Some(id) = name.strip_suffix("-search.tar.gz") {
        (id, "search")
    } else if let Some(id) = name.strip_suffix("-routing.tar.gz") {
        (id, "routing")
    } else {
        bail!("{name} is not a ReflectMaps pack (expected <state>-map.pmtiles, <state>-search.tar.gz or <state>-routing.tar.gz)");
    };
    let cat: Option<Catalog> = std::fs::read(state.paths.catalog_cache()).ok().and_then(|b| serde_json::from_slice(&b).ok());
    let reg = cat.as_ref().and_then(|c| c.regions.iter().find(|r| r.id == id).cloned());
    let region_name = reg.as_ref().map(|r| r.name.clone()).unwrap_or_else(|| id.to_uppercase());
    let sha = sha256_file(path)?;
    let size = std::fs::metadata(path)?.len();
    // Work on a copy so the user's file stays where it is.
    let copy = state.paths.downloads().join(format!("{name}.import"));
    std::fs::copy(path, &copy)?;
    let tag = cat
        .as_ref()
        .filter(|_| reg.as_ref().and_then(|r| r.components.get(kind)).is_some_and(|x| x.sha256 == sha))
        .map(|c| c.tag.clone())
        .unwrap_or_else(|| "imported".into());
    install_file(state, id, &region_name, reg.and_then(|r| r.bbox), kind, &copy, &tag, (&name, size, &sha))?;
    if kind == "routing" {
        crate::routing::refresh(state);
    }
    Ok(format!("{region_name} {kind}"))
}
