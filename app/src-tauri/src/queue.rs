//! The download queue: whole states downloaded a few at a time, for
//! "Download selected", "Download all" and "Update all".
//!
//! The list of waiting states is saved in the data folder, so closing the
//! app or unplugging the drive loses nothing; it comes back paused, and
//! partly downloaded files carry on where they stopped.

use crate::packs::{self, Catalog};
use crate::state::AppState;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// States downloading at the same time.
const PARALLEL: usize = 2;
/// Attempts per state before it is marked failed.
const ATTEMPTS: u32 = 3;

#[derive(Debug, Default, Serialize, Deserialize)]
struct Saved {
    items: Vec<String>,
    #[serde(default)]
    sizes: BTreeMap<String, u64>,
}

#[derive(Debug, Default)]
struct Inner {
    /// Waiting and active states, in order.
    items: Vec<String>,
    active: BTreeSet<String>,
    paused: bool,
    failed: BTreeMap<String, String>,
    attempts: BTreeMap<String, u32>,
    /// Bytes to download for each queued state, worked out when queued.
    sizes: BTreeMap<String, u64>,
    /// Totals for the current run, for the overall progress bar.
    batch_total: u64,
    batch_done: u64,
    batch_states: usize,
    batch_states_done: usize,
    errors_in_a_row: u32,
    note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QueueStatus {
    pub items: Vec<String>,
    pub active: Vec<String>,
    pub paused: bool,
    pub failed: BTreeMap<String, String>,
    pub batch_total: u64,
    pub batch_done: u64,
    pub batch_states: usize,
    pub batch_states_done: usize,
    pub note: Option<String>,
}

#[derive(Default)]
pub struct Queue {
    inner: Mutex<Inner>,
    wake: tokio::sync::Notify,
}

/// Parts of a state that are missing or differ from the catalog.
pub fn needed(state: &AppState, cat: &Catalog, region: &str) -> Vec<(String, u64)> {
    let Some(r) = cat.regions.iter().find(|r| r.id == region) else { return Vec::new() };
    let inst = state.installed.lock().unwrap();
    let have = inst.regions.get(region);
    r.components
        .iter()
        .filter(|(k, c)| have.and_then(|h| h.components.get(*k)).is_none_or(|h| h.sha256 != c.sha256))
        .map(|(k, c)| (k.clone(), c.size))
        .collect()
}

impl Queue {
    fn save(&self, state: &AppState) {
        let saved = {
            let q = self.inner.lock().unwrap();
            Saved { items: q.items.clone(), sizes: q.sizes.clone() }
        };
        let path = state.paths.queue_file();
        if saved.items.is_empty() {
            let _ = std::fs::remove_file(path);
        } else if let Ok(b) = serde_json::to_vec_pretty(&saved) {
            let _ = std::fs::write(path, b);
        }
    }

    /// Loads a queue left from last time. It starts paused, so the app never
    /// begins a big download by itself.
    pub fn load(&self, state: &AppState) {
        let Ok(b) = std::fs::read(state.paths.queue_file()) else { return };
        let Ok(saved) = serde_json::from_slice::<Saved>(&b) else { return };
        if saved.items.is_empty() {
            return;
        }
        let mut q = self.inner.lock().unwrap();
        q.batch_total = saved.items.iter().map(|i| saved.sizes.get(i).copied().unwrap_or(0)).sum();
        q.batch_states = saved.items.len();
        q.items = saved.items;
        q.sizes = saved.sizes;
        q.paused = true;
        q.note = Some(format!(
            "{} state{} still to download from last time. Press Resume to continue.",
            q.items.len(),
            if q.items.len() == 1 { "" } else { "s" }
        ));
    }

    pub fn status(&self) -> QueueStatus {
        let q = self.inner.lock().unwrap();
        QueueStatus {
            items: q.items.clone(),
            active: q.active.iter().cloned().collect(),
            paused: q.paused,
            failed: q.failed.clone(),
            batch_total: q.batch_total,
            batch_done: q.batch_done,
            batch_states: q.batch_states,
            batch_states_done: q.batch_states_done,
            note: q.note.clone(),
        }
    }

    fn changed(&self, app: &AppHandle, state: &AppState) {
        self.save(state);
        let _ = app.emit("queue", self.status());
        self.wake.notify_one();
    }

    pub fn add(&self, app: &AppHandle, state: &AppState, cat: &Catalog, regions: &[String]) -> usize {
        let mut regions = regions.to_vec();
        // Directions only join up at state lines when built together, so a
        // directions update also queues installed states on an older build.
        if regions.iter().any(|id| needed(state, cat, id).iter().any(|(k, _)| k == "routing")) {
            let inst = state.installed.lock().unwrap();
            for (id, r) in &inst.regions {
                if r.components.get("routing").is_some_and(|c| c.graph_build != cat.routing_graph) && !regions.contains(id) {
                    regions.push(id.clone());
                }
            }
        }
        let mut added = 0;
        {
            let mut q = self.inner.lock().unwrap();
            if q.items.is_empty() {
                // a new run
                q.batch_total = 0;
                q.batch_done = 0;
                q.batch_states = 0;
                q.batch_states_done = 0;
            }
            for id in &regions {
                if q.items.contains(id) {
                    continue;
                }
                let need: u64 = needed(state, cat, id).iter().map(|(_, s)| s).sum();
                if need == 0 {
                    continue;
                }
                q.failed.remove(id);
                q.attempts.remove(id);
                q.items.push(id.clone());
                q.sizes.insert(id.clone(), need);
                q.batch_total += need;
                q.batch_states += 1;
                added += 1;
            }
            if added > 0 {
                q.paused = false;
                q.note = None;
                q.errors_in_a_row = 0;
            }
        }
        self.changed(app, state);
        added
    }

    pub fn remove(&self, app: &AppHandle, state: &AppState, regions: &[String]) {
        {
            let mut q = self.inner.lock().unwrap();
            for id in regions {
                if let Some(pos) = q.items.iter().position(|x| x == id) {
                    q.items.remove(pos);
                    let size = q.sizes.remove(id).unwrap_or(0);
                    q.batch_total = q.batch_total.saturating_sub(size);
                    q.batch_states = q.batch_states.saturating_sub(1);
                }
                q.failed.remove(id);
                if q.active.contains(id) {
                    cancel(state, id);
                }
            }
        }
        self.changed(app, state);
    }

    pub fn pause(&self, app: &AppHandle, state: &AppState) {
        {
            let mut q = self.inner.lock().unwrap();
            q.paused = true;
            q.note = None;
            for id in &q.active {
                cancel(state, id);
            }
        }
        self.changed(app, state);
    }

    pub fn resume(&self, app: &AppHandle, state: &AppState) {
        {
            let mut q = self.inner.lock().unwrap();
            q.paused = false;
            q.note = None;
            q.errors_in_a_row = 0;
        }
        self.changed(app, state);
    }

    pub fn clear_failed(&self, app: &AppHandle, state: &AppState) {
        self.inner.lock().unwrap().failed.clear();
        self.changed(app, state);
    }

    /// Runs for the life of the app, starting queued states as slots free up.
    pub async fn run(app: AppHandle) {
        let state = app.state::<Arc<AppState>>().inner().clone();
        loop {
            let next = {
                let mut q = state.queue.inner.lock().unwrap();
                if q.paused || q.active.len() >= PARALLEL {
                    None
                } else {
                    let id = q.items.iter().find(|i| !q.active.contains(*i)).cloned();
                    if let Some(id) = &id {
                        q.active.insert(id.clone());
                    }
                    id
                }
            };
            match next {
                Some(id) => {
                    let _ = app.emit("queue", state.queue.status());
                    let (app2, st) = (app.clone(), state.clone());
                    tauri::async_runtime::spawn(async move {
                        let res = one(&app2, &st, &id).await;
                        finished(&app2, &st, &id, res).await;
                    });
                }
                None => state.queue.wake.notified().await,
            }
        }
    }
}

fn cancel(state: &AppState, id: &str) {
    let mut c = state.cancel.lock().unwrap();
    for k in ["map", "search", "routing"] {
        c.insert(format!("{id}:{k}"));
    }
}

async fn one(app: &AppHandle, state: &Arc<AppState>, id: &str) -> Result<()> {
    let cat = packs::get_catalog(state, false)
        .await
        .catalog
        .ok_or_else(|| anyhow::anyhow!("no list of states yet; connect to the internet once"))?;
    let need = needed(state, &cat, id);
    if need.is_empty() {
        return Ok(());
    }
    // Room for the download plus the unpacked copy.
    let bytes: u64 = need.iter().map(|(_, s)| s).sum();
    if let Some(free) = crate::storage::free_space(&state.paths.data) {
        let want = bytes * 2 + 512 * 1024 * 1024;
        if free < want {
            anyhow::bail!(
                "not enough space on the drive ({} free, about {} needed)",
                crate::storage::format_bytes(free),
                crate::storage::format_bytes(want)
            );
        }
    }
    {
        let mut busy = state.busy.lock().unwrap();
        if !busy.insert(id.to_string()) {
            anyhow::bail!("already downloading");
        }
    }
    let comps: Vec<String> = need.into_iter().map(|(k, _)| k).collect();
    let r = packs::install_inner(app, state, id, &comps, false).await;
    state.busy.lock().unwrap().remove(id);
    r
}

async fn finished(app: &AppHandle, state: &Arc<AppState>, id: &str, res: Result<()>) {
    let q = &state.queue;
    let mut retry_after = None;
    {
        let mut g = q.inner.lock().unwrap();
        g.active.remove(id);
        match &res {
            Ok(()) => {
                g.items.retain(|x| x != id);
                let size = g.sizes.remove(id).unwrap_or(0);
                g.batch_done += size;
                g.batch_states_done += 1;
                g.attempts.remove(id);
                g.errors_in_a_row = 0;
            }
            Err(e) if e.to_string() == "cancelled" => {} // paused or removed: stays as it is
            Err(e) => {
                let msg = format!("{e:#}");
                let n = g.attempts.entry(id.to_string()).or_insert(0);
                *n += 1;
                let n = *n;
                g.errors_in_a_row += 1;
                if msg.starts_with("not enough space") {
                    g.paused = true;
                    g.note = Some(format!("Downloads paused: {msg}. Free up space, then press Resume."));
                } else if g.errors_in_a_row >= 4 {
                    g.paused = true;
                    g.note = Some(format!(
                        "Downloads paused after repeated errors ({msg}). Check the internet connection, then press Resume."
                    ));
                } else if n >= ATTEMPTS {
                    g.items.retain(|x| x != id);
                    let size = g.sizes.remove(id).unwrap_or(0);
                    g.batch_total = g.batch_total.saturating_sub(size);
                    g.batch_states = g.batch_states.saturating_sub(1);
                    g.failed.insert(id.to_string(), msg);
                } else {
                    // try again shortly, after the other waiting states
                    if let Some(pos) = g.items.iter().position(|x| x == id) {
                        let item = g.items.remove(pos);
                        g.items.push(item);
                    }
                    retry_after = Some(Duration::from_secs(10 * n as u64));
                }
            }
        }
        if g.items.is_empty() {
            g.note = None;
        }
    }
    if let Some(d) = retry_after {
        // Hold the slot briefly so a flaky connection isn't hammered.
        q.inner.lock().unwrap().active.insert(id.to_string());
        let _ = app.emit("queue", q.status());
        tokio::time::sleep(d).await;
        q.inner.lock().unwrap().active.remove(id);
    }
    if res.is_ok() {
        let _ = app.emit("packs-changed", id);
    }
    q.changed(app, state);
}
