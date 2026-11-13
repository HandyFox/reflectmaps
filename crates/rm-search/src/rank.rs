//! Re-ranking of raw index matches.
//!
//! Tantivy finds candidates; this module decides their order. The score
//! combines how much of the query a result covers (with typo tolerance and
//! prefix matching for the word being typed), whether the name itself
//! matches, house-number agreement, distance from the map view, the kind of
//! result and its importance.

use crate::normalize::{self, is_house_number, is_stop_word};
use crate::Fields;
use serde::Serialize;
use tantivy::schema::{TantivyDocument, Value};

#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub label: String,
    pub category: String,
    pub lat: f64,
    pub lon: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub distance_km: Option<f64>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub phone: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub website: String,
    pub source: String,
    pub score: f64,
    /// True when the position is estimated rather than an exact match
    /// (e.g. a house number not found on the matched street).
    pub approximate: bool,
    #[serde(skip)]
    pub(crate) norm: String,
    #[serde(skip)]
    pub(crate) number: String,
    #[serde(skip)]
    pub(crate) importance: f64,
}

pub(crate) struct ParsedQuery {
    /// Normalized tokens, in order.
    pub tokens: Vec<String>,
    /// Indexes into `tokens` that carry meaning (not stop words).
    pub meaningful: Vec<usize>,
    pub house_number: Option<String>,
    /// True when the user has finished the last word (typed a space).
    pub ends_with_space: bool,
    pub joined: String,
}

impl ParsedQuery {
    pub fn new(q: &str) -> Self {
        let tokens = normalize::tokens(q);
        let mut meaningful: Vec<usize> = (0..tokens.len()).filter(|&i| !is_stop_word(&tokens[i])).collect();
        if meaningful.is_empty() {
            meaningful = (0..tokens.len()).collect();
        }
        let house_number = tokens
            .first()
            .filter(|t| is_house_number(t) && tokens.len() > 1)
            .cloned();
        let joined = meaningful.iter().map(|&i| tokens[i].as_str()).collect::<Vec<_>>().join(" ");
        Self {
            ends_with_space: q.ends_with(' '),
            tokens,
            meaningful,
            house_number,
            joined,
        }
    }
}

pub(crate) fn fuzzy_distance(tok: &str) -> u8 {
    if tok.chars().all(|c| c.is_ascii_digit()) {
        return 0;
    }
    match tok.len() {
        0..=3 => 0,
        4..=7 => 1,
        _ => 2,
    }
}

fn get_str(doc: &TantivyDocument, f: tantivy::schema::Field) -> String {
    doc.get_first(f).and_then(|v| v.as_str()).unwrap_or_default().to_string()
}

fn get_f64(doc: &TantivyDocument, f: tantivy::schema::Field) -> f64 {
    doc.get_first(f).and_then(|v| v.as_f64()).unwrap_or_default()
}

pub(crate) fn to_hit(f: &Fields, doc: &TantivyDocument, bm25: f32) -> Hit {
    Hit {
        id: get_str(doc, f.id),
        kind: get_str(doc, f.kind),
        name: get_str(doc, f.name),
        label: get_str(doc, f.label),
        category: get_str(doc, f.category),
        lat: get_f64(doc, f.lat),
        lon: get_f64(doc, f.lon),
        distance_km: None,
        phone: get_str(doc, f.phone),
        website: get_str(doc, f.website),
        source: get_str(doc, f.source),
        score: bm25 as f64,
        approximate: false,
        norm: get_str(doc, f.norm),
        number: get_str(doc, f.number),
        importance: get_f64(doc, f.importance),
    }
}

pub fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6371.0088;
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = (lat2 - lat1).to_radians();
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * r * a.sqrt().asin()
}

/// Edit distance counting a swap of two neighboring letters as one edit,
/// matching how the index's fuzzy queries count typos.
fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for j in 0..=m {
        d[0][j] = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[n][m]
}

/// How well one query token matches a list of document tokens (0..1).
fn token_match(tok: &str, is_last_open: bool, doc: &[&str]) -> f64 {
    let dist = fuzzy_distance(tok) as usize;
    let mut best: f64 = 0.0;
    for d in doc {
        if *d == tok {
            return 1.0;
        }
        if is_last_open && tok.len() >= 1 && d.starts_with(tok) {
            best = best.max(0.97);
            continue;
        }
        if dist > 0 && d.len().abs_diff(tok.len()) <= dist && levenshtein(tok, d) <= dist {
            best = best.max(0.8);
        }
        if is_last_open && tok.len() >= 5 && d.len() > tok.len() {
            let prefix: String = d.chars().take(tok.chars().count()).collect();
            if levenshtein(tok, &prefix) <= 1 {
                best = best.max(0.7);
            }
        }
    }
    if best < 0.9 {
        for syn in normalize::category_synonyms(tok) {
            let words: Vec<&str> = syn.split(' ').collect();
            if words.iter().all(|w| doc.contains(w)) {
                best = best.max(0.9);
            }
        }
    }
    best
}

fn kind_weight(kind: &str) -> f64 {
    match kind {
        "state" => 1.4,
        "city" | "town" => 1.25,
        "village" => 1.15,
        "county" => 1.05,
        "zip" => 1.1,
        "place" => 1.0,
        "address" => 1.0,
        "street" => 0.85,
        "neighborhood" => 0.95,
        _ => 0.9,
    }
}

fn is_area(kind: &str) -> bool {
    matches!(kind, "state" | "city" | "town" | "village" | "county" | "zip" | "neighborhood")
}

pub(crate) fn rerank(mut hits: Vec<Hit>, q: &ParsedQuery, near: Option<(f64, f64)>, limit: usize) -> Vec<Hit> {
    let n_meaningful = q.meaningful.len() as f64;
    let last_idx = q.tokens.len() - 1;
    for h in hits.iter_mut() {
        let doc_tokens: Vec<&str> = h.norm.split(' ').collect();
        let name_norm = normalize::normalize(&h.name);
        let name_tokens: Vec<&str> = name_norm.split(' ').collect();

        let mut cov = 0.0;
        let mut name_cov = 0.0;
        for &i in &q.meaningful {
            let tok = &q.tokens[i];
            let open = i == last_idx && !q.ends_with_space;
            cov += token_match(tok, open, &doc_tokens);
            name_cov += token_match(tok, open, &name_tokens);
        }
        cov /= n_meaningful;
        name_cov /= n_meaningful;

        let mut text = cov.powi(4) * (0.55 + 0.45 * name_cov);
        if name_norm == q.joined {
            text += 0.6;
        } else if name_norm.starts_with(&q.joined) {
            text += 0.25;
        }

        // House numbers: an address with the right number wins; a wrong
        // number on the right street drops well below it.
        let mut num_factor = 1.0;
        if let Some(num) = &q.house_number {
            match h.kind.as_str() {
                "address" if &h.number == num => num_factor = 2.2,
                "address" => num_factor = 0.35,
                "street" => {
                    num_factor = 0.9;
                    h.approximate = true;
                }
                "place" => num_factor = 0.8,
                _ => num_factor = 0.6,
            }
        }

        // Category searches ("coffee", "gas") are about what is close by;
        // named searches can reach further.
        let category_intent = name_cov < 0.5 && cov >= 0.75 && !is_area(&h.kind);
        let mut dist_factor = 1.0;
        if let Some((lat, lon)) = near {
            let d = haversine_km(lat, lon, h.lat, h.lon);
            h.distance_km = Some(d);
            let mut scale = if category_intent {
                4.0
            } else if is_area(&h.kind) {
                400.0
            } else if h.kind == "address" || h.kind == "street" {
                60.0
            } else {
                40.0
            };
            // When every word of the query matches (e.g. it names the city),
            // the user has said where they mean; distance matters less.
            if !category_intent && cov >= 0.97 && q.meaningful.len() >= 2 {
                scale *= 8.0;
            }
            dist_factor = 1.0 / (1.0 + d / scale).powf(0.8);
        }

        let imp = 0.6 + 0.6 * h.importance;
        let mut score = text * num_factor * dist_factor * kind_weight(&h.kind) * imp;
        if cov < 0.6 {
            score = 0.0;
        }
        h.score = score;
    }
    hits.retain(|h| h.score > 0.0);
    hits.sort_by(|a, b| b.score.total_cmp(&a.score));

    // Drop near-duplicates. Map data often lists one venue several times
    // with slightly different addresses ("Churchill Downs" at 700 Central
    // Ave and at 600 Terminal Dr). Same name within 600 m counts as the same
    // place; the best-ranked copy is kept. Addresses and streets only merge
    // when they are the same text.
    let mut out: Vec<Hit> = Vec::with_capacity(limit);
    for h in hits {
        let dup = out.iter().any(|o| {
            let d = haversine_km(o.lat, o.lon, h.lat, h.lon);
            let same_name = normalize::normalize(&o.name) == normalize::normalize(&h.name);
            let place_like = !matches!(h.kind.as_str(), "address" | "street");
            same_name && (d < 0.15 && o.label.eq_ignore_ascii_case(&h.label) || place_like && d < 0.6)
        }) || out.iter().any(|o| o.id == h.id);
        if !dup {
            out.push(h);
        }
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// "41.4993, -81.6944" or "41.4993 -81.6944".
pub(crate) fn parse_coordinates(q: &str) -> Option<Hit> {
    let cleaned = q.trim().replace(',', " ");
    let parts: Vec<&str> = cleaned.split_whitespace().collect();
    if parts.len() != 2 || !parts.iter().all(|p| p.contains('.')) {
        return None;
    }
    let lat: f64 = parts[0].parse().ok()?;
    let lon: f64 = parts[1].parse().ok()?;
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return None;
    }
    Some(Hit {
        id: format!("coord:{lat},{lon}"),
        kind: "coordinates".into(),
        name: format!("{lat:.5}, {lon:.5}"),
        label: "Coordinates".into(),
        category: String::new(),
        lat,
        lon,
        distance_km: None,
        phone: String::new(),
        website: String::new(),
        source: String::new(),
        score: 1.0,
        approximate: false,
        norm: String::new(),
        number: String::new(),
        importance: 1.0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typo_matches() {
        assert!(token_match("walgrens", false, &["walgreens"]) >= 0.8);
        assert!(token_match("mcdonlds", false, &["mcdonalds"]) >= 0.8);
        assert_eq!(token_match("cof", true, &["coffee"]), 0.97);
        assert_eq!(token_match("cof", false, &["coffee"]), 0.0);
    }

    #[test]
    fn coordinates() {
        assert!(parse_coordinates("41.4993, -81.6944").is_some());
        assert!(parse_coordinates("123 456").is_none());
    }
}
