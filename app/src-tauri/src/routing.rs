//! Driving directions with Valhalla.
//!
//! The app ships Valhalla's `valhalla_service` program and runs it once per
//! request ("one-shot" mode): no background server, no network port. The
//! route graph is read from the tiles unpacked out of routing packs.

use crate::state::AppState;
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct Valhalla {
    exe: PathBuf,
    lib_dirs: Vec<PathBuf>,
    config: PathBuf,
}

/// Finds the bundled Valhalla runtime and writes its config. Called at
/// startup and whenever routing packs change.
pub fn refresh(state: &AppState) {
    match Valhalla::setup(state) {
        Ok(v) => {
            *state.routing.lock().unwrap() = Some(v);
            *state.routing_error.lock().unwrap() = None;
        }
        Err(e) => {
            eprintln!("routing unavailable: {e:#}");
            *state.routing.lock().unwrap() = None;
            *state.routing_error.lock().unwrap() = Some(format!("{e:#}"));
        }
    }
}

impl Valhalla {
    fn setup(state: &AppState) -> Result<Self> {
        let runtime = state.paths.resources.join("valhalla-runtime");
        let exe_name = if cfg!(windows) { "valhalla_service.exe" } else { "valhalla_service" };
        let mut exe = runtime.join("valhalla").join("bin").join(exe_name);
        if !exe.exists() {
            bail!("routing engine is not bundled with this build ({})", exe.display());
        }
        #[cfg(unix)]
        {
            exe = ensure_executable(&runtime, &exe, &state.paths.data)?;
        }
        let root = exe.parent().unwrap().parent().unwrap().parent().unwrap().to_path_buf();
        let lib_dirs = vec![
            root.join("pyvalhalla.libs"),
            root.join("valhalla").join(".dylibs"),
            root.join("valhalla").join("bin"),
        ];
        let defaults: Value = serde_json::from_slice(
            &std::fs::read(state.paths.resources.join("valhalla-defaults.json")).context("valhalla-defaults.json")?,
        )?;
        let mut cfg = defaults;
        let tiles = state.paths.routing_tiles();
        std::fs::create_dir_all(&tiles)?;
        cfg["mjolnir"]["tile_dir"] = json!(tiles.to_string_lossy());
        cfg["mjolnir"]["tile_extract"] = json!("");
        cfg["mjolnir"]["traffic_extract"] = json!("");
        cfg["mjolnir"]["admin"] = json!("");
        cfg["mjolnir"]["timezone"] = json!("");
        cfg["mjolnir"]["logging"] = json!({"type": "", "color": false});
        cfg["loki"]["logging"] = json!({"type": "", "color": false, "long_request": 100.0});
        cfg["thor"]["logging"] = json!({"type": "", "color": false, "long_request": 110.0});
        cfg["odin"]["logging"] = json!({"type": "", "color": false});
        cfg["meili"]["logging"] = json!({"type": "", "color": false});
        cfg["additional_data"]["elevation"] = json!("");
        let config = state.paths.routing().join("valhalla.json");
        std::fs::write(&config, serde_json::to_vec_pretty(&cfg)?)?;
        Ok(Self { exe, lib_dirs, config })
    }

    pub fn request(&self, action: &str, body: &Value) -> Result<Value> {
        let mut cmd = Command::new(&self.exe);
        cmd.arg(&self.config).arg(action).arg(body.to_string());
        let sep = if cfg!(windows) { ";" } else { ":" };
        let libs: Vec<String> = self.lib_dirs.iter().filter(|d| d.exists()).map(|d| d.to_string_lossy().into()).collect();
        let joined = libs.join(sep);
        if cfg!(windows) {
            let path = std::env::var("PATH").unwrap_or_default();
            cmd.env("PATH", format!("{joined}{sep}{path}"));
        } else if cfg!(target_os = "macos") {
            cmd.env("DYLD_LIBRARY_PATH", &joined);
        } else {
            cmd.env("LD_LIBRARY_PATH", &joined);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let out = cmd.output().with_context(|| format!("starting {}", self.exe.display()))?;
        let stdout = String::from_utf8_lossy(&out.stdout);
        let text = stdout.trim();
        let parsed: Option<Value> = text.find('{').and_then(|i| serde_json::from_str(&text[i..]).ok());
        match parsed {
            Some(v) if v.get("error_code").is_some() || v.get("error").is_some() => Err(friendly_error(&v)),
            Some(v) => Ok(v),
            None => {
                let err = String::from_utf8_lossy(&out.stderr);
                bail!("routing engine failed: {}", err.lines().last().unwrap_or("no output"))
            }
        }
    }
}

#[cfg(unix)]
fn ensure_executable(runtime: &Path, exe: &Path, data: &Path) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let mode = std::fs::metadata(exe)?.permissions().mode();
    if mode & 0o111 != 0 {
        return Ok(exe.to_path_buf());
    }
    if std::fs::set_permissions(exe, std::fs::Permissions::from_mode(0o755)).is_ok() {
        return Ok(exe.to_path_buf());
    }
    // Read-only install location: copy the runtime somewhere writable.
    let dest = data.join("valhalla-runtime");
    copy_dir(runtime, &dest)?;
    let new_exe = dest.join("valhalla").join("bin").join("valhalla_service");
    std::fs::set_permissions(&new_exe, std::fs::Permissions::from_mode(0o755))?;
    Ok(new_exe)
}

#[cfg(unix)]
fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let to = dst.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &to)?;
        } else {
            std::fs::copy(e.path(), &to)?;
        }
    }
    Ok(())
}

fn friendly_error(v: &Value) -> anyhow::Error {
    let code = v["error_code"].as_i64().unwrap_or(0);
    let msg = v["error"].as_str().unwrap_or("unknown error");
    let text = match code {
        170 | 171 => "No road was found near one of your points. Make sure the routing pack for that area is installed.".to_string(),
        154 | 155 | 156 | 157 => "That route is longer than the routing engine allows.".to_string(),
        442 => "No driving route connects these points.".to_string(),
        _ => format!("Routing failed: {msg}"),
    };
    anyhow!(text)
}

// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
pub struct LatLon {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouteRequest {
    pub locations: Vec<LatLon>,
    #[serde(default)]
    pub avoid_tolls: bool,
    #[serde(default)]
    pub avoid_highways: bool,
    #[serde(default)]
    pub avoid_ferries: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Maneuver {
    pub instruction: String,
    pub distance_mi: f64,
    pub time_s: f64,
    #[serde(rename = "type")]
    pub kind: i64,
    pub street_names: Vec<String>,
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RouteOption {
    pub id: usize,
    pub label: String,
    pub via: String,
    pub distance_mi: f64,
    pub time_s: f64,
    pub has_toll: bool,
    pub has_highway: bool,
    pub has_ferry: bool,
    pub warnings: Vec<String>,
    /// [lon, lat] pairs
    pub coordinates: Vec<[f64; 2]>,
    pub maneuvers: Vec<Maneuver>,
}

/// Decodes a polyline with 6 digits of precision into [lon, lat] pairs.
fn decode_polyline6(s: &str) -> Vec<[f64; 2]> {
    let bytes = s.as_bytes();
    let (mut i, mut lat, mut lon) = (0usize, 0i64, 0i64);
    let mut out = Vec::new();
    while i < bytes.len() {
        for coord in 0..2 {
            let (mut result, mut shift) = (0i64, 0);
            loop {
                if i >= bytes.len() {
                    return out;
                }
                let b = bytes[i] as i64 - 63;
                i += 1;
                result |= (b & 0x1f) << shift;
                shift += 5;
                if b < 0x20 {
                    break;
                }
            }
            let delta = if result & 1 != 0 { !(result >> 1) } else { result >> 1 };
            if coord == 0 {
                lat += delta;
            } else {
                lon += delta;
            }
        }
        out.push([lon as f64 / 1e6, lat as f64 / 1e6]);
    }
    out
}

fn trip_to_option(trip: &Value, label: &str, req: &RouteRequest) -> RouteOption {
    let mut coordinates = Vec::new();
    let mut maneuvers = Vec::new();
    let legs = trip["legs"].as_array().cloned().unwrap_or_default();
    let nlegs = legs.len();
    for (li, leg) in legs.iter().enumerate() {
        let shape = decode_polyline6(leg["shape"].as_str().unwrap_or(""));
        for m in leg["maneuvers"].as_array().cloned().unwrap_or_default() {
            let idx = m["begin_shape_index"].as_u64().unwrap_or(0) as usize;
            let pt = shape.get(idx).copied().unwrap_or([0.0, 0.0]);
            let mut instruction = m["instruction"].as_str().unwrap_or("").to_string();
            let kind = m["type"].as_i64().unwrap_or(0);
            // Destination maneuvers of intermediate legs are stops.
            if li + 1 < nlegs && (4..=6).contains(&kind) {
                instruction = format!("Arrive at stop {}", li + 1);
            }
            maneuvers.push(Maneuver {
                instruction,
                distance_mi: m["length"].as_f64().unwrap_or(0.0),
                time_s: m["time"].as_f64().unwrap_or(0.0),
                kind,
                street_names: m["street_names"]
                    .as_array()
                    .map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect())
                    .unwrap_or_default(),
                lat: pt[1],
                lon: pt[0],
            });
        }
        if !coordinates.is_empty() && !shape.is_empty() {
            coordinates.extend_from_slice(&shape[1..]);
        } else {
            coordinates.extend(shape);
        }
    }
    let s = &trip["summary"];
    let mut warnings = Vec::new();
    let (has_toll, has_highway, has_ferry) = (
        s["has_toll"].as_bool().unwrap_or(false),
        s["has_highway"].as_bool().unwrap_or(false),
        s["has_ferry"].as_bool().unwrap_or(false),
    );
    if req.avoid_tolls && has_toll {
        warnings.push("Uses a toll road: no toll-free route was found".into());
    }
    if req.avoid_highways && has_highway {
        warnings.push("Uses a highway: no highway-free route was found".into());
    }
    if req.avoid_ferries && has_ferry {
        warnings.push("Uses a ferry: no ferry-free route was found".into());
    }
    RouteOption {
        id: 0,
        label: label.to_string(),
        via: String::new(),
        distance_mi: s["length"].as_f64().unwrap_or(0.0),
        time_s: s["time"].as_f64().unwrap_or(0.0),
        has_toll,
        has_highway,
        has_ferry,
        warnings,
        coordinates,
        maneuvers,
    }
}

fn base_request(req: &RouteRequest, alternates: u32, avoid_highways: bool, shortest: bool) -> Value {
    // Every point is a stop ("break"), so each stop starts a new leg.
    let locations: Vec<Value> = req.locations.iter().map(|l| json!({"lat": l.lat, "lon": l.lon, "type": "break"})).collect();
    let mut auto = json!({
        "use_tolls": if req.avoid_tolls { 0.0 } else { 0.5 },
        "use_highways": if avoid_highways { 0.0 } else { 1.0 },
        "use_ferry": if req.avoid_ferries { 0.0 } else { 0.5 },
    });
    if shortest {
        auto["shortest"] = json!(true);
    }
    let mut body = json!({
        "locations": locations,
        "costing": "auto",
        "costing_options": {"auto": auto},
        "directions_options": {"units": "miles", "language": "en-US"},
    });
    if alternates > 0 {
        body["alternates"] = json!(alternates);
    }
    body
}

/// Cells of roughly 100 m the route passes through, for overlap checks.
fn cells(r: &RouteOption) -> HashSet<(i64, i64)> {
    r.coordinates.iter().map(|c| ((c[0] * 1000.0).round() as i64, (c[1] * 1000.0).round() as i64)).collect()
}

fn road_share(r: &RouteOption) -> HashMap<String, f64> {
    let mut m = HashMap::new();
    for man in &r.maneuvers {
        if let Some(n) = man.street_names.first() {
            *m.entry(n.clone()).or_insert(0.0) += man.distance_mi;
        }
    }
    m
}

pub fn plan(v: &Valhalla, req: &RouteRequest) -> Result<Vec<RouteOption>> {
    if req.locations.len() < 2 {
        bail!("Choose a start and a destination.");
    }
    let two = req.locations.len() == 2;
    // Candidate requests run in parallel: the main request with Valhalla's
    // own alternates, plus a shortest-distance route and a no-highway route.
    let mut jobs: Vec<(&str, Value)> = vec![("main", base_request(req, if two { 2 } else { 0 }, req.avoid_highways, false))];
    if two {
        jobs.push(("shortest", base_request(req, 0, req.avoid_highways, true)));
        if !req.avoid_highways {
            jobs.push(("nohighway", base_request(req, 0, true, false)));
        }
    }
    let results: Vec<(&str, Result<Value>)> = std::thread::scope(|s| {
        let handles: Vec<_> = jobs.iter().map(|(k, body)| (*k, s.spawn(move || v.request("route", body)))).collect();
        handles.into_iter().map(|(k, h)| (k, h.join().unwrap_or_else(|_| Err(anyhow!("routing crashed"))))).collect()
    });

    let mut options: Vec<RouteOption> = Vec::new();
    let mut main_err = None;
    for (kind, res) in results {
        match res {
            Ok(resp) => {
                if kind == "main" {
                    options.push(trip_to_option(&resp["trip"], "Fastest", req));
                    for alt in resp["alternates"].as_array().cloned().unwrap_or_default() {
                        options.push(trip_to_option(&alt["trip"], "Alternative", req));
                    }
                } else if kind == "shortest" {
                    options.push(trip_to_option(&resp["trip"], "Shortest", req));
                } else {
                    let mut o = trip_to_option(&resp["trip"], "No highways", req);
                    o.warnings.retain(|w| !w.starts_with("Uses a highway"));
                    options.push(o);
                }
            }
            Err(e) if kind == "main" => main_err = Some(e),
            Err(_) => {}
        }
    }
    if let Some(e) = main_err {
        return Err(e);
    }

    // Drop candidates that mostly repeat one already kept.
    let mut kept: Vec<(RouteOption, HashSet<(i64, i64)>)> = Vec::new();
    for o in options {
        let c = cells(&o);
        let dup = kept.iter().any(|(_, k)| {
            let inter = c.intersection(k).count() as f64;
            let union = c.union(k).count().max(1) as f64;
            inter / union > 0.8
        });
        // "Shortest" only matters if it is noticeably shorter than the fastest.
        let pointless = o.label == "Shortest"
            && kept.first().is_some_and(|(f, _)| o.distance_mi > f.distance_mi * 0.97);
        if !dup && !pointless {
            kept.push((o, c));
        }
    }
    let fastest_roads = kept.first().map(|(f, _)| road_share(f)).unwrap_or_default();
    let fastest_len = kept.first().map(|(f, _)| f.distance_mi).unwrap_or(1.0).max(0.01);
    let mut out: Vec<RouteOption> = kept.into_iter().map(|(o, _)| o).collect();
    for (i, o) in out.iter_mut().enumerate() {
        o.id = i;
        // Name each route after its main road that the fastest route does not use much.
        let mut roads: Vec<(String, f64)> = road_share(o).into_iter().collect();
        roads.sort_by(|a, b| b.1.total_cmp(&a.1));
        let distinct = roads
            .iter()
            .find(|(n, _)| fastest_roads.get(n).copied().unwrap_or(0.0) / fastest_len < 0.15)
            .or(roads.first());
        if let Some((n, _)) = distinct {
            o.via = n.clone();
        }
    }
    if out.len() == 1 {
        out[0].label = "Best route".into();
    }
    Ok(out)
}
