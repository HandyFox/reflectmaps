mod packs;
mod pmtiles;
mod routing;
mod state;

use serde::Serialize;
use state::{AppState, Installed, Paths};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Manager, State};

type St<'a> = State<'a, Arc<AppState>>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

#[derive(Serialize)]
struct Status {
    version: String,
    data_dir: String,
    installed: Installed,
    has_world: bool,
    has_areas: bool,
    routing_ready: bool,
    routing_error: Option<String>,
    attribution: Vec<String>,
}

#[tauri::command]
fn app_status(state: St) -> Status {
    let tiles = state.tiles.read().unwrap();
    let search = state.search.read().unwrap();
    Status {
        version: env!("CARGO_PKG_VERSION").into(),
        data_dir: state.paths.data.to_string_lossy().into(),
        installed: state.installed.lock().unwrap().clone(),
        has_world: tiles.iter().any(|(id, _)| id == "world"),
        has_areas: search.index_names().iter().any(|n| n == "us-areas"),
        routing_ready: state.routing.lock().unwrap().is_some(),
        routing_error: state.routing_error.lock().unwrap().clone(),
        attribution: tiles
            .first()
            .and_then(|(_, t)| t.metadata().ok())
            .and_then(|m| m["attribution"].as_str().map(String::from))
            .into_iter()
            .collect(),
    }
}

/// Returns one vector tile (uncompressed MVT bytes), or an empty body.
#[tauri::command]
async fn get_tile(state: St<'_>, z: u8, x: u32, y: u32) -> Result<tauri::ipc::Response, String> {
    let tiles: Vec<_> = state.tiles.read().unwrap().iter().map(|(_, t)| t.clone()).collect();
    for t in tiles {
        if !t.may_contain(z, x, y) {
            continue;
        }
        if let Ok(Some(data)) = t.get(z, x, y) {
            return Ok(tauri::ipc::Response::new(data));
        }
    }
    Ok(tauri::ipc::Response::new(Vec::new()))
}

#[tauri::command]
async fn search(state: St<'_>, query: String, near: Option<(f64, f64)>, limit: Option<usize>) -> Result<Vec<rm_search::Hit>, String> {
    let st = state.inner().clone();
    tokio::task::spawn_blocking(move || {
        st.search
            .read()
            .unwrap()
            .search(&query, rm_search::QueryOptions { near, limit: limit.unwrap_or(8) })
            .map_err(err)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn reverse(state: St<'_>, lat: f64, lon: f64) -> Result<Option<rm_search::Hit>, String> {
    let st = state.inner().clone();
    tokio::task::spawn_blocking(move || st.search.read().unwrap().reverse(lat, lon).map_err(err))
        .await
        .map_err(err)?
}

#[tauri::command]
async fn route(state: St<'_>, request: routing::RouteRequest) -> Result<Vec<routing::RouteOption>, String> {
    let st = state.inner().clone();
    let has_tiles = st.installed.lock().unwrap().regions.values().any(|r| r.components.contains_key("routing"));
    if !has_tiles {
        return Err("Download the routing pack for this area first (Downloads tab).".into());
    }
    tokio::task::spawn_blocking(move || {
        let guard = st.routing.lock().unwrap();
        let v = guard.as_ref().ok_or_else(|| {
            st.routing_error.lock().unwrap().clone().unwrap_or_else(|| "Routing is not available.".into())
        })?;
        routing::plan(v, &request).map_err(err)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn get_catalog(state: St<'_>, refresh: bool) -> Result<packs::CatalogResult, String> {
    Ok(packs::get_catalog(state.inner(), refresh).await)
}

#[tauri::command]
async fn install_region(app: tauri::AppHandle, region: String, components: Vec<String>) -> Result<(), String> {
    packs::install(app, region, components).await.map_err(|e| format!("{e:#}"))
}

#[tauri::command]
fn cancel_install(state: St, region: String) {
    let mut c = state.cancel.lock().unwrap();
    for k in ["map", "search", "routing"] {
        c.insert(format!("{region}:{k}"));
    }
}

#[tauri::command]
async fn remove_region(state: St<'_>, region: String) -> Result<(), String> {
    let st = state.inner().clone();
    tokio::task::spawn_blocking(move || packs::remove(&st, &region).map_err(err)).await.map_err(err)?
}

#[tauri::command]
async fn import_pack(state: St<'_>, path: String) -> Result<String, String> {
    let st = state.inner().clone();
    tokio::task::spawn_blocking(move || packs::import(&st, &PathBuf::from(path)).map_err(|e| format!("{e:#}")))
        .await
        .map_err(err)?
}

pub fn run() {
    // WebKitGTK's DMA-BUF renderer shows a blank window on some NVIDIA
    // drivers; the plain path is reliable everywhere.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data = match std::env::var_os("RM_DATA_DIR") {
                Some(d) => PathBuf::from(d),
                None => app.path().app_local_data_dir()?.join("data"),
            };
            let resources = match std::env::var_os("RM_RESOURCE_DIR") {
                Some(d) => PathBuf::from(d),
                None => app.path().resource_dir()?.join("resources"),
            };
            let state = Arc::new(AppState::new(Paths { data, resources }));
            state.load();
            routing::refresh(&state);
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_status,
            get_tile,
            search,
            reverse,
            route,
            get_catalog,
            install_region,
            cancel_install,
            remove_region,
            import_pack
        ])
        .run(tauri::generate_context!())
        .expect("error while running ReflectMaps");
}
