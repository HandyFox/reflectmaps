//! Query one or more indexes from the command line.
//!
//! Usage: rm-search-query [--near LAT,LON] [--limit N] [--reverse] INDEX_DIR... -- QUERY

use anyhow::{bail, Result};
use rm_search::{QueryOptions, Searcher};
use std::path::Path;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut near = None;
    let mut limit = 10;
    let mut reverse = false;
    let mut dirs = Vec::new();
    let mut query = String::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--near" => {
                let v: Vec<f64> = args[i + 1].split(',').map(|s| s.trim().parse().unwrap()).collect();
                near = Some((v[0], v[1]));
                i += 1;
            }
            "--limit" => {
                limit = args[i + 1].parse()?;
                i += 1;
            }
            "--reverse" => reverse = true,
            "--" => {
                query = args[i + 1..].join(" ");
                break;
            }
            d => dirs.push(d.to_string()),
        }
        i += 1;
    }
    if dirs.is_empty() || query.is_empty() {
        bail!("usage: rm-search-query [--near LAT,LON] [--limit N] [--reverse] INDEX_DIR... -- QUERY");
    }
    let mut s = Searcher::new();
    for d in &dirs {
        s.add_index(d, Path::new(d))?;
    }
    if reverse {
        let v: Vec<f64> = query.split(',').map(|s| s.trim().parse().unwrap()).collect();
        println!("{}", serde_json::to_string_pretty(&s.reverse(v[0], v[1])?)?);
        return Ok(());
    }
    let t = std::time::Instant::now();
    let hits = s.search(&query, QueryOptions { near, limit })?;
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    for (n, h) in hits.iter().enumerate() {
        println!(
            "{:>2}. [{:<7}] {} | {} | {:.5},{:.5} | score {:.3}{}{}",
            n + 1,
            h.kind,
            h.name,
            h.label,
            h.lat,
            h.lon,
            h.score,
            h.distance_km.map(|d| format!(" | {d:.1} km")).unwrap_or_default(),
            if h.approximate { " | approximate" } else { "" }
        );
    }
    eprintln!("{} results in {ms:.1} ms", hits.len());
    Ok(())
}
