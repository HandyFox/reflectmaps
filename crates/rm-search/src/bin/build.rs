//! Build a search index from newline-delimited JSON records.
//!
//! Usage: rm-search-build <records.ndjson | -> <index-dir>

use anyhow::{bail, Context, Result};
use rm_search::{IndexBuilder, Record};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        bail!("usage: rm-search-build <records.ndjson | -> <index-dir>");
    }
    let input: Box<dyn BufRead> = if args[0] == "-" {
        Box::new(BufReader::new(std::io::stdin()))
    } else {
        Box::new(BufReader::new(std::fs::File::open(&args[0]).with_context(|| args[0].clone())?))
    };
    let out = PathBuf::from(&args[1]);
    if out.exists() {
        std::fs::remove_dir_all(&out)?;
    }
    let mut b = IndexBuilder::create(&out)?;
    let mut skipped = 0u64;
    for (n, line) in input.lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Record>(&line) {
            Ok(r) if r.lat.is_finite() && r.lon.is_finite() && !r.name.trim().is_empty() => b.add(&r)?,
            Ok(_) => skipped += 1,
            Err(e) => {
                skipped += 1;
                if skipped <= 5 {
                    eprintln!("line {}: {e}", n + 1);
                }
            }
        }
    }
    let count = b.finish()?;
    println!("indexed {count} records into {} ({skipped} skipped)", out.display());
    Ok(())
}
