mod check;
mod import;
mod packs;
mod queue;
mod storage;
mod userdata;
mod pmtiles;
mod routing;
mod state;

use serde::Serialize;
use state::{AppState, Installed, Paths};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Emitter, Manager, State};

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

#[derive(Clone, serde::Serialize)]
struct SearchProgress {
    channel: String,
    seq: u64,
    done: usize,
    total: usize,
}

/// Search. `states` limits it to those states' packs (cities and ZIP codes
/// are kept only inside them); `None` searches everything installed.
/// Each search box is a `channel`: a newer `seq` on the same channel
/// cancels an older search still running. Progress arrives as
/// "search-progress" events.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
async fn search(
    app: tauri::AppHandle,
    state: St<'_>,
    query: String,
    near: Option<(f64, f64)>,
    limit: Option<usize>,
    states: Option<Vec<String>>,
    channel: Option<String>,
    seq: Option<u64>,
) -> Result<Vec<rm_search::Hit>, String> {
    let st = state.inner().clone();
    let channel = channel.unwrap_or_else(|| "default".into());
    let seq = seq.unwrap_or(0);
    st.search_seq.lock().unwrap().insert(channel.clone(), seq);
    let scope = match states {
        None => rm_search::Scope::default(),
        Some(ids) => {
            let inst = st.installed.lock().unwrap();
            let boxes = ids.iter().filter_map(|id| inst.regions.get(id).and_then(|r| r.bbox)).collect();
            rm_search::Scope {
                indexes: Some(ids.iter().map(|id| format!("{id}-search")).collect()),
                shared: vec!["us-areas".into()],
                boxes,
            }
        }
    };
    tokio::task::spawn_blocking(move || {
        let last = std::sync::Mutex::new(std::time::Instant::now());
        let res = st.search.read().unwrap().search_scoped(
            &query,
            rm_search::QueryOptions { near, limit: limit.unwrap_or(8) },
            &scope,
            &|done, total| {
                // at most ~10 updates a second
                let mut l = last.lock().unwrap();
                if done == 0 || done == total || l.elapsed().as_millis() > 100 {
                    *l = std::time::Instant::now();
                    let _ = app.emit("search-progress", SearchProgress { channel: channel.clone(), seq, done, total });
                }
            },
            &|| st.search_seq.lock().unwrap().get(&channel).is_some_and(|s| *s != seq),
        );
        res.map_err(|e| if e.is::<rm_search::Cancelled>() { "cancelled".to_string() } else { format!("{e:#}") })
    })
    .await
    .map_err(err)?
}

#[tauri::command]
async fn reverse(state: St<'_>, lat: f64, lon: f64) -> Result<Option<rm_search::Hit>, String> {
    let st = state.inner().clone();
    // Only the states around the point (and the national areas list).
    let only: Vec<String> = {
        let inst = st.installed.lock().unwrap();
        let pad = 0.05;
        let mut v: Vec<String> = inst
            .regions
            .iter()
            .filter(|(_, r)| {
                r.bbox.is_none_or(|b| lon >= b[0] - pad && lat >= b[1] - pad && lon <= b[2] + pad && lat <= b[3] + pad)
            })
            .map(|(id, _)| format!("{id}-search"))
            .collect();
        v.push("us-areas".into());
        v
    };
    tokio::task::spawn_blocking(move || st.search.read().unwrap().reverse_in(lat, lon, Some(&only)).map_err(err))
        .await
        .map_err(err)?
}

#[derive(Clone, serde::Serialize)]
struct RouteProgress {
    done: usize,
    total: usize,
}

#[tauri::command]
async fn route(app: tauri::AppHandle, state: St<'_>, request: routing::RouteRequest) -> Result<Vec<routing::RouteOption>, String> {
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
        routing::plan_with(v, &request, &|done, total| {
            let _ = app.emit("route-progress", RouteProgress { done, total });
        })
        .map_err(err)
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

/// Installs pack files (or folders of them) the user already has on disk.
#[tauri::command]
async fn import_packs(app: tauri::AppHandle, state: St<'_>, paths: Vec<String>) -> Result<import::ImportReport, String> {
    let st = state.inner().clone();
    let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
    tokio::task::spawn_blocking(move || import::import(&|p| packs::emit(&app, p), &st, &paths).map_err(|e| format!("{e:#}")))
        .await
        .map_err(err)?
}

#[tauri::command]
fn storage_info(state: St) -> storage::StorageInfo {
    storage::info(&state.paths.data, state.portable)
}

#[tauri::command]
fn queue_status(state: St) -> queue::QueueStatus {
    state.queue.status()
}

/// Queues whole states for download (only the parts they are missing or
/// that changed). Returns how many were queued.
#[tauri::command]
async fn queue_add(app: tauri::AppHandle, state: St<'_>, regions: Vec<String>) -> Result<usize, String> {
    let cat = packs::get_catalog(state.inner(), false)
        .await
        .catalog
        .ok_or("No list of states yet. Connect to the internet once.")?;
    Ok(state.queue.add(&app, state.inner(), &cat, &regions))
}

#[tauri::command]
fn queue_remove(app: tauri::AppHandle, state: St, regions: Vec<String>) {
    state.queue.remove(&app, state.inner(), &regions);
}

#[tauri::command]
fn queue_pause(app: tauri::AppHandle, state: St) {
    state.queue.pause(&app, state.inner());
}

#[tauri::command]
fn queue_resume(app: tauri::AppHandle, state: St) {
    state.queue.resume(&app, state.inner());
}

#[tauri::command]
fn queue_clear_failed(app: tauri::AppHandle, state: St) {
    state.queue.clear_failed(&app, state.inner());
}

/// Checks every installed file; progress arrives as "check-progress" events.
#[tauri::command]
async fn check_data(app: tauri::AppHandle, state: St<'_>) -> Result<Vec<check::RegionCheck>, String> {
    let st = state.inner().clone();
    st.cancel.lock().unwrap().remove("check");
    tokio::task::spawn_blocking(move || {
        check::check_all(
            &st,
            &|p| {
                let _ = app.emit("check-progress", p);
            },
            &|| st.cancel.lock().unwrap().remove("check"),
        )
        .map_err(|e| format!("{e:#}"))
    })
    .await
    .map_err(err)?
}

#[tauri::command]
fn cancel_check(state: St) {
    state.cancel.lock().unwrap().insert("check".into());
}

/// Address history ("history") or address book ("places") as JSON.
#[tauri::command]
fn user_get(state: St, kind: String) -> Result<serde_json::Value, String> {
    userdata::get(&state, &kind).map_err(err)
}

#[tauri::command]
fn user_set(state: St, kind: String, value: serde_json::Value) -> Result<(), String> {
    userdata::set(&state, &kind, &value).map_err(err)
}

/// Deletes address history and the address book.
#[tauri::command]
fn user_clear(state: St) -> Result<(), String> {
    userdata::clear(&state).map_err(err)
}

#[tauri::command]
fn places_export(state: St, path: String) -> Result<usize, String> {
    userdata::export_csv(&state, &PathBuf::from(path)).map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn places_import(state: St<'_>, path: String) -> Result<userdata::ImportReport, String> {
    let st = state.inner().clone();
    tokio::task::spawn_blocking(move || userdata::import_csv(&st, &PathBuf::from(path)).map_err(|e| format!("{e:#}")))
        .await
        .map_err(err)?
}

/// Opens the system print dialog for the window. `window.print()` alone does
/// nothing in the macOS web view, so printing goes through the native call.
#[tauri::command]
fn print_page(webview: tauri::Webview) -> Result<(), String> {
    webview.print().map_err(err)
}

pub fn run() {
    // WebKitGTK's DMA-BUF renderer shows a blank window on some NVIDIA
    // drivers; the plain path is reliable everywhere.
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    let portable = storage::portable_root();
    if let Some(root) = &portable {
        storage::setup_portable_env(root);
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            let data = match (std::env::var_os("RM_DATA_DIR"), &portable) {
                (Some(d), _) => PathBuf::from(d),
                (None, Some(root)) => root.join(storage::PORTABLE_DATA),
                (None, None) => app.path().app_local_data_dir()?.join("data"),
            };
            let resources = match (std::env::var_os("RM_RESOURCE_DIR"), &portable) {
                (Some(d), _) => PathBuf::from(d),
                (None, Some(root)) => root.join("resources"),
                (None, None) => app.path().resource_dir()?.join("resources"),
            };
            let mut st = AppState::new(Paths { data: data.clone(), resources });
            st.portable = portable.is_some();
            let state = Arc::new(st);
            state.load();
            state.queue.load(&state);
            routing::refresh(&state);
            app.manage(state);

            // The window is made here so a portable copy keeps the web view's
            // storage (settings, saved map view) on its own drive.
            let cfg = app.config().app.windows.first().cloned().ok_or("no window in tauri.conf.json")?;
            let mut win = tauri::WebviewWindowBuilder::from_config(app.handle(), &cfg)?;
            if portable.is_some() {
                win = win.data_directory(data.join("webview"));
            }
            win.build()?;

            tauri::async_runtime::spawn(queue::Queue::run(app.handle().clone()));
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
            import_packs,
            print_page,
            storage_info,
            queue_status,
            queue_add,
            queue_remove,
            queue_pause,
            queue_resume,
            queue_clear_failed,
            check_data,
            cancel_check,
            user_get,
            user_set,
            user_clear,
            places_export,
            places_import
        ])
        .run(tauri::generate_context!())
        .expect("error while running ReflectMaps");
}
