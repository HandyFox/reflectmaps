//! Measure search accuracy against a file of test cases.
//!
//! Usage: rm-search-eval --cases cases.json [--min-pass 0.9] INDEX_DIR...
//!
//! A case passes when one of the top N results (default 5) is within
//! `within_km` of the expected position and, if given, its name contains
//! `name` (case-insensitive). Exits non-zero if the pass rate is below
//! --min-pass, so CI can block on it.

use anyhow::{bail, Context, Result};
use rm_search::{QueryOptions, Searcher};
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
struct Expect {
    #[serde(default)]
    name: Option<String>,
    lat: f64,
    lon: f64,
    #[serde(default = "default_km")]
    within_km: f64,
}

fn default_km() -> f64 {
    2.0
}

#[derive(Deserialize)]
struct Case {
    query: String,
    #[serde(default)]
    near: Option<(f64, f64)>,
    expect: Expect,
    #[serde(default = "default_top")]
    top: usize,
    #[serde(default)]
    note: String,
}

fn default_top() -> usize {
    5
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut cases_path = None;
    let mut min_pass = 0.0;
    let mut dirs = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--cases" => {
                cases_path = Some(args[i + 1].clone());
                i += 1;
            }
            "--min-pass" => {
                min_pass = args[i + 1].parse()?;
                i += 1;
            }
            d => dirs.push(d.to_string()),
        }
        i += 1;
    }
    let Some(cases_path) = cases_path else { bail!("--cases is required") };
    let cases: Vec<Case> =
        serde_json::from_str(&std::fs::read_to_string(&cases_path).context("reading cases")?)?;
    let mut s = Searcher::new();
    for d in &dirs {
        s.add_index(d, Path::new(d))?;
    }
    let (mut pass, mut first) = (0usize, 0usize);
    let mut total_ms = 0.0;
    for c in &cases {
        let t = std::time::Instant::now();
        let hits = s.search(&c.query, QueryOptions { near: c.near, limit: c.top.max(5) })?;
        total_ms += t.elapsed().as_secs_f64() * 1000.0;
        let ok_at = hits.iter().take(c.top).position(|h| {
            let d = rm_search::normalize::normalize(&h.name);
            let name_ok = c
                .expect
                .name
                .as_ref()
                .map_or(true, |n| d.contains(&rm_search::normalize::normalize(n)));
            name_ok && haversine(h.lat, h.lon, c.expect.lat, c.expect.lon) <= c.expect.within_km
        });
        match ok_at {
            Some(p) => {
                pass += 1;
                if p == 0 {
                    first += 1;
                }
                println!("PASS #{} {:<40} -> {}", p + 1, c.query, hits[p].name);
            }
            None => {
                let got = hits.first().map(|h| format!("{} ({})", h.name, h.label)).unwrap_or("nothing".into());
                println!("FAIL    {:<40} -> {got}  {}", c.query, c.note);
            }
        }
    }
    let n = cases.len().max(1) as f64;
    let rate = pass as f64 / n;
    println!(
        "\n{pass}/{} passed in top results ({:.0}%), {first} ranked first ({:.0}%), {:.1} ms average",
        cases.len(),
        rate * 100.0,
        first as f64 / n * 100.0,
        total_ms / n
    );
    if rate < min_pass {
        bail!("pass rate {:.0}% is below the required {:.0}%", rate * 100.0, min_pass * 100.0);
    }
    Ok(())
}

fn haversine(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6371.0088;
    let dp = (lat2 - lat1).to_radians();
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + lat1.to_radians().cos() * lat2.to_radians().cos() * (dl / 2.0).sin().powi(2);
    2.0 * r * a.sqrt().asin()
}
