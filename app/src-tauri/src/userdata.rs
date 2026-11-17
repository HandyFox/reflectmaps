//! The user's own data: address history and address book (saved places).
//!
//! Kept as JSON files in `<data>/user/`, so on a portable drive they travel
//! with it. "Clear personal data" deletes the folder, e.g. before copying a
//! drive to give away. The app treats the contents as opaque JSON except
//! for CSV import and export of the address book.

use crate::state::AppState;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};

const KINDS: [&str; 2] = ["history", "places"];

fn dir(state: &AppState) -> PathBuf {
    state.paths.data.join("user")
}

fn file(state: &AppState, kind: &str) -> Result<PathBuf> {
    if !KINDS.contains(&kind) {
        bail!("unknown user data '{kind}'");
    }
    Ok(dir(state).join(format!("{kind}.json")))
}

pub fn get(state: &AppState, kind: &str) -> Result<Value> {
    match std::fs::read(file(state, kind)?) {
        Ok(b) => Ok(serde_json::from_slice(&b).unwrap_or(Value::Array(vec![]))),
        Err(_) => Ok(Value::Array(vec![])),
    }
}

/// Written to a temporary file first, so a pulled drive never leaves half a file.
pub fn set(state: &AppState, kind: &str, value: &Value) -> Result<()> {
    let path = file(state, kind)?;
    std::fs::create_dir_all(dir(state))?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn clear(state: &AppState) -> Result<()> {
    let d = dir(state);
    if d.exists() {
        std::fs::remove_dir_all(&d)?;
    }
    Ok(())
}

/// One address book entry, as the app stores it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Saved {
    pub id: String,
    /// The user's name for it ("Mom's house").
    pub name: String,
    pub hit: rm_search::Hit,
    pub added: f64,
}

const HEADER: [&str; 8] = ["name", "address", "latitude", "longitude", "category", "phone", "website", "place name"];

pub fn export_csv(state: &AppState, path: &Path) -> Result<usize> {
    let places: Vec<Saved> = serde_json::from_value(get(state, "places")?).unwrap_or_default();
    let mut w = csv::Writer::from_path(path).with_context(|| format!("writing {}", path.display()))?;
    w.write_record(HEADER)?;
    for p in &places {
        let h = &p.hit;
        let address = if h.kind == "address" && h.label.is_empty() { h.name.clone() } else { address_of(h) };
        w.write_record([
            p.name.as_str(),
            address.as_str(),
            &format!("{:.6}", h.lat),
            &format!("{:.6}", h.lon),
            h.category.as_str(),
            h.phone.as_str(),
            h.website.as_str(),
            h.name.as_str(),
        ])?;
    }
    w.flush()?;
    Ok(places.len())
}

/// The street address of a hit as one line.
fn address_of(h: &rm_search::Hit) -> String {
    match h.kind.as_str() {
        // "123 Main St" + "Columbus, OH 43215"
        "address" => format!("{}, {}", h.name, h.label),
        // places carry their address as the label
        _ => h.label.clone(),
    }
}

#[derive(Debug, Default, Serialize)]
pub struct ImportReport {
    pub added: usize,
    /// Rows found by looking up their address.
    pub looked_up: usize,
    pub skipped: Vec<String>,
}

/// Adds the rows of a CSV file to the address book. Columns are matched by
/// header (name, address, latitude/lat, longitude/lon/lng, ...); rows
/// without coordinates are looked up by address in the installed states.
pub fn import_csv(state: &AppState, path: &Path) -> Result<ImportReport> {
    let mut r = csv::ReaderBuilder::new()
        .flexible(true)
        .trim(csv::Trim::All)
        .from_path(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let headers: Vec<String> = r.headers()?.iter().map(|h| h.trim_start_matches('\u{feff}').to_lowercase()).collect();
    let col = |names: &[&str]| headers.iter().position(|h| names.contains(&h.as_str()));
    let c_name = col(&["name", "title", "label"]);
    let c_addr = col(&["address", "full address", "street address", "location"]);
    let c_lat = col(&["latitude", "lat"]);
    let c_lon = col(&["longitude", "lon", "lng", "long"]);
    let c_cat = col(&["category"]);
    let c_phone = col(&["phone", "phone number", "telephone"]);
    let c_web = col(&["website", "url", "web"]);
    let c_place = col(&["place name"]);
    if c_addr.is_none() && (c_lat.is_none() || c_lon.is_none()) {
        bail!("the file needs an 'address' column, or 'latitude' and 'longitude' columns");
    }

    let mut places: Vec<Saved> = serde_json::from_value(get(state, "places")?).unwrap_or_default();
    let mut report = ImportReport::default();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0);

    for (n, rec) in r.records().enumerate() {
        let rec = match rec {
            Ok(x) => x,
            Err(e) => {
                report.skipped.push(format!("row {}: {e}", n + 2));
                continue;
            }
        };
        let get = |c: Option<usize>| c.and_then(|i| rec.get(i)).unwrap_or("").to_string();
        let name = get(c_name);
        let addr = get(c_addr);
        let coords = (get(c_lat).parse::<f64>(), get(c_lon).parse::<f64>());
        let hit = match coords {
            (Ok(lat), Ok(lon)) if (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) => {
                let mut h = rm_search::Hit::default();
                h.id = format!("csv:{lat:.6},{lon:.6}");
                h.kind = "place".into();
                let p = get(c_place);
                h.name = if !p.is_empty() { p } else if !name.is_empty() { name.clone() } else { addr.clone() };
                h.label = addr.clone();
                h.category = get(c_cat);
                h.lat = lat;
                h.lon = lon;
                h.phone = get(c_phone);
                h.website = get(c_web);
                h.source = "csv".into();
                h
            }
            _ => {
                let q = if !addr.is_empty() { addr.clone() } else { name.clone() };
                if q.is_empty() {
                    report.skipped.push(format!("row {}: no address or coordinates", n + 2));
                    continue;
                }
                let hits = state
                    .search
                    .read()
                    .unwrap()
                    .search(&q, rm_search::QueryOptions { near: None, limit: 1 })
                    .unwrap_or_default();
                match hits.into_iter().next() {
                    Some(mut h) => {
                        report.looked_up += 1;
                        let p = get(c_phone);
                        if !p.is_empty() {
                            h.phone = p;
                        }
                        h
                    }
                    None => {
                        report.skipped.push(format!("row {}: “{q}” not found in the installed states", n + 2));
                        continue;
                    }
                }
            }
        };
        let label = if !name.is_empty() { name } else { hit.name.clone() };
        // Same name at (nearly) the same spot is already in the book.
        if places.iter().any(|p| p.name == label && (p.hit.lat - hit.lat).abs() < 1e-4 && (p.hit.lon - hit.lon).abs() < 1e-4) {
            continue;
        }
        places.push(Saved { id: format!("p{}-{}", now as u64, places.len()), name: label, hit, added: now });
        report.added += 1;
    }
    set(state, "places", &serde_json::to_value(&places)?)?;
    Ok(report)
}

/// CSV round trip against real data:
/// RM_CSV_TEST_DATA=<data dir with a state installed> cargo test -p reflectmaps --lib userdata::e2e -- --ignored --nocapture
#[cfg(test)]
mod e2e {
    use super::*;
    use crate::state::Paths;
    use std::sync::Arc;

    #[test]
    #[ignore]
    fn csv_round_trip() {
        let data = PathBuf::from(std::env::var("RM_CSV_TEST_DATA").unwrap());
        let resources = Path::new(env!("CARGO_MANIFEST_DIR")).join("resources");
        let state = Arc::new(AppState::new(Paths { data, resources }));
        state.load();
        clear(&state).unwrap();
        let tmp = std::env::temp_dir().join("rm-book.csv");
        std::fs::write(
            &tmp,
            "\u{feff}Name,Address,Lat,Lng,Phone\n\
             Office,,39.158100,-75.524400,302-555-0100\n\
             \"Smith, Jo\",779 N Dupont Hwy Dover,,,\n\
             Nowhere,1 Zzyzx Qqq Road,,,\n\
             ,,,,\n",
        )
        .unwrap();
        let r = import_csv(&state, &tmp).unwrap();
        eprintln!("import: {}", serde_json::to_string(&r).unwrap());
        let again = import_csv(&state, &tmp).unwrap();
        eprintln!("again: added {}", again.added);
        let out = std::env::temp_dir().join("rm-book-out.csv");
        let n = export_csv(&state, &out).unwrap();
        eprintln!("exported {n}:\n{}", std::fs::read_to_string(&out).unwrap());
        clear(&state).unwrap();
    }
}
