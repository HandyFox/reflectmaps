//! ReflectMaps offline search.
//!
//! One index per pack (a state's places and addresses, or the national list
//! of cities and ZIP codes). [`Searcher`] queries any number of them at once
//! and merges the results with a ranking that weighs text match, distance
//! from the map view and importance.

pub mod normalize;
mod rank;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, BoostQuery, FuzzyTermQuery, Occur, Query, RangeQuery, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, Schema, TantivyDocument, FAST, INDEXED, STORED, STRING, TEXT,
};
use tantivy::{Index, IndexReader, ReloadPolicy, Term};

pub use rank::Hit;

/// One searchable record, as produced by the data pipeline (one JSON object
/// per line).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Record {
    pub id: String,
    /// place, address, street, city, town, village, county, state, zip, neighborhood
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    /// Second display line, e.g. "123 Main St, Columbus, OH 43215".
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub category: String,
    /// Words describing the category and its parents ("food and drink restaurant pizza").
    #[serde(default)]
    pub category_words: String,
    #[serde(default)]
    pub number: String,
    #[serde(default)]
    pub street: String,
    #[serde(default)]
    pub city: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub zip: String,
    pub lat: f64,
    pub lon: f64,
    /// 0..1, higher is more prominent.
    #[serde(default)]
    pub importance: f64,
    #[serde(default)]
    pub phone: String,
    #[serde(default)]
    pub website: String,
    #[serde(default)]
    pub source: String,
}

#[derive(Clone, Copy)]
pub(crate) struct Fields {
    pub id: Field,
    pub kind: Field,
    pub name: Field,
    pub label: Field,
    pub category: Field,
    pub text: Field,
    pub name_text: Field,
    pub norm: Field,
    pub number: Field,
    pub zip: Field,
    pub lat: Field,
    pub lon: Field,
    pub importance: Field,
    pub phone: Field,
    pub website: Field,
    pub source: Field,
}

fn schema() -> (Schema, Fields) {
    let mut b = Schema::builder();
    let f = Fields {
        id: b.add_text_field("id", STRING | STORED),
        kind: b.add_text_field("kind", STRING | STORED),
        name: b.add_text_field("name", STORED),
        label: b.add_text_field("label", STORED),
        category: b.add_text_field("category", STRING | STORED),
        text: b.add_text_field("text", TEXT),
        name_text: b.add_text_field("name_text", TEXT),
        norm: b.add_text_field("norm", STORED),
        number: b.add_text_field("number", STRING | STORED),
        zip: b.add_text_field("zip", STRING | STORED),
        lat: b.add_f64_field("lat", INDEXED | STORED | FAST),
        lon: b.add_f64_field("lon", INDEXED | STORED | FAST),
        importance: b.add_f64_field("importance", STORED | FAST),
        phone: b.add_text_field("phone", STORED),
        website: b.add_text_field("website", STORED),
        source: b.add_text_field("source", STORED),
    };
    (b.build(), f)
}

fn fields_of(schema: &Schema) -> Result<Fields> {
    let g = |n: &str| schema.get_field(n).with_context(|| format!("index is missing field {n}"));
    Ok(Fields {
        id: g("id")?,
        kind: g("kind")?,
        name: g("name")?,
        label: g("label")?,
        category: g("category")?,
        text: g("text")?,
        name_text: g("name_text")?,
        norm: g("norm")?,
        number: g("number")?,
        zip: g("zip")?,
        lat: g("lat")?,
        lon: g("lon")?,
        importance: g("importance")?,
        phone: g("phone")?,
        website: g("website")?,
        source: g("source")?,
    })
}

/// Builds an index directory from records.
pub struct IndexBuilder {
    writer: tantivy::IndexWriter,
    f: Fields,
    count: u64,
}

impl IndexBuilder {
    pub fn create(dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(dir)?;
        let (schema, f) = schema();
        let index = Index::create_in_dir(dir, schema).context("creating index")?;
        let writer = index.writer(400_000_000)?;
        Ok(Self { writer, f, count: 0 })
    }

    pub fn add(&mut self, r: &Record) -> Result<()> {
        let mut r = r.clone();
        r.name = if matches!(r.kind.as_str(), "address" | "street") {
            normalize::fix_case_words(&r.name)
        } else {
            normalize::fix_case(&r.name)
        };
        r.label = normalize::fix_state_codes(&normalize::fix_case_words(&normalize::fix_case(&r.label)));
        r.street = normalize::fix_case_words(&r.street);
        r.city = normalize::fix_case_words(&r.city);
        let r = &r;
        let f = &self.f;
        let mut name_words = normalize::normalize(&r.name);
        for a in &r.aliases {
            name_words.push(' ');
            name_words.push_str(&normalize::normalize(a));
        }
        // "St. Marys" is also found as "Saint Marys".
        if name_words.starts_with("street ") {
            name_words.push_str(" saint ");
            name_words.push_str(&name_words["street ".len()..].to_string());
        }
        let mut text = name_words.clone();
        for part in [
            r.category_words.as_str(),
            &normalize::category_words(&r.category),
            &r.number,
            &r.street,
            &r.city,
            &r.state,
            &r.zip,
            &r.label,
        ] {
            if !part.is_empty() {
                text.push(' ');
                text.push_str(&normalize::normalize(part));
            }
        }
        let mut doc = TantivyDocument::default();
        doc.add_text(f.id, &r.id);
        doc.add_text(f.kind, &r.kind);
        doc.add_text(f.name, &r.name);
        doc.add_text(f.label, &r.label);
        doc.add_text(f.category, &r.category);
        doc.add_text(f.text, &text);
        doc.add_text(f.name_text, &name_words);
        doc.add_text(f.norm, &text);
        if !r.number.is_empty() {
            doc.add_text(f.number, normalize::normalize(&r.number));
        }
        if !r.zip.is_empty() {
            doc.add_text(f.zip, &r.zip);
        }
        doc.add_f64(f.lat, r.lat);
        doc.add_f64(f.lon, r.lon);
        doc.add_f64(f.importance, r.importance.clamp(0.0, 1.0));
        if !r.phone.is_empty() {
            doc.add_text(f.phone, &r.phone);
        }
        if !r.website.is_empty() {
            doc.add_text(f.website, &r.website);
        }
        doc.add_text(f.source, &r.source);
        self.writer.add_document(doc)?;
        self.count += 1;
        Ok(())
    }

    pub fn finish(mut self) -> Result<u64> {
        self.writer.commit()?;
        self.writer.wait_merging_threads()?;
        Ok(self.count)
    }
}

struct OpenIndex {
    name: String,
    reader: IndexReader,
    f: Fields,
}

/// Searches one or more indexes at once.
#[derive(Default)]
pub struct Searcher {
    indexes: Vec<OpenIndex>,
}

/// Options for a query.
#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub struct QueryOptions {
    /// Center of the map view; results nearby rank higher.
    pub near: Option<(f64, f64)>,
    pub limit: usize,
}

/// Which indexes a query reads.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    /// Names of the indexes to search; `None` searches all of them.
    pub indexes: Option<Vec<String>>,
    /// Indexes covering the whole country (cities, ZIP codes): searched even
    /// when not listed in `indexes`, keeping only results inside `boxes`.
    pub shared: Vec<String>,
    /// [west, south, east, north] areas for results from `shared` indexes.
    pub boxes: Vec<[f64; 4]>,
}

impl Scope {
    /// Whether to read index `name`, and if so whether to keep its results
    /// only inside `boxes`.
    fn wants(&self, name: &str) -> Option<bool> {
        match &self.indexes {
            None => Some(false),
            Some(list) if list.iter().any(|n| n == name) => Some(false),
            Some(_) if self.shared.iter().any(|n| n == name) => Some(true),
            Some(_) => None,
        }
    }
}

/// Candidates read in full from each search, across all indexes.
const WIDE: usize = 600;
const CLOSE: usize = 150;

/// Raised when a newer query replaced this one before it finished.
#[derive(Debug)]
pub struct Cancelled;
impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cancelled")
    }
}
impl std::error::Error for Cancelled {}

fn in_boxes(boxes: &[[f64; 4]], lat: f64, lon: f64) -> bool {
    boxes.iter().any(|b| lon >= b[0] && lat >= b[1] && lon <= b[2] && lat <= b[3])
}

impl Searcher {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an index directory. `name` identifies it (e.g. "oh-search").
    pub fn add_index(&mut self, name: &str, dir: &Path) -> Result<()> {
        let index = Index::open_in_dir(dir).with_context(|| format!("opening index {}", dir.display()))?;
        let f = fields_of(&index.schema())?;
        let reader = index.reader_builder().reload_policy(ReloadPolicy::Manual).try_into()?;
        self.indexes.retain(|i| i.name != name);
        self.indexes.push(OpenIndex { name: name.to_string(), reader, f });
        Ok(())
    }

    pub fn remove_index(&mut self, name: &str) {
        self.indexes.retain(|i| i.name != name);
    }

    pub fn index_names(&self) -> Vec<String> {
        self.indexes.iter().map(|i| i.name.clone()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.indexes.is_empty()
    }

    /// Free-text search for places, addresses, streets and areas.
    pub fn search(&self, query: &str, opts: QueryOptions) -> Result<Vec<Hit>> {
        self.search_scoped(query, opts, &Scope::default(), &|_, _| {}, &|| false)
    }

    /// Like [`search`](Self::search), limited to `scope`. `progress(done,
    /// total)` is called as indexes finish; when `cancelled()` turns true the
    /// search stops with a [`Cancelled`] error.
    pub fn search_scoped(
        &self,
        query: &str,
        opts: QueryOptions,
        scope: &Scope,
        progress: &(dyn Fn(usize, usize) + Sync),
        cancelled: &(dyn Fn() -> bool + Sync),
    ) -> Result<Vec<Hit>> {
        let limit = if opts.limit == 0 { 10 } else { opts.limit };
        if let Some(hit) = rank::parse_coordinates(query) {
            return Ok(vec![hit]);
        }
        let parsed = rank::ParsedQuery::new(query);
        if parsed.tokens.is_empty() {
            return Ok(vec![]);
        }

        // Indexes to read, biggest first so the work spreads evenly.
        let mut todo: Vec<(usize, bool)> = self
            .indexes
            .iter()
            .enumerate()
            .filter_map(|(i, idx)| scope.wants(&idx.name).map(|clip| (i, clip)))
            .collect();
        todo.sort_by_key(|(i, _)| std::cmp::Reverse(self.indexes[*i].reader.searcher().num_docs()));
        let total = todo.len();
        progress(0, total);

        // Candidates from each index, in parallel, as (score, index, doc)
        // without reading the documents: reading stored documents is the
        // slow part, so only the best candidates overall are read below.
        // The second, nearby-only pass finds close places for category
        // searches ("gas station" -> the Speedway around the corner).
        let near = opts.near;
        let next = std::sync::atomic::AtomicUsize::new(0);
        let done = std::sync::atomic::AtomicUsize::new(0);
        type Cand = (f32, usize, tantivy::DocAddress);
        let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 8).min(total.max(1));
        let results: Vec<Result<(Vec<Cand>, Vec<Cand>)>> = std::thread::scope(|sc| {
            let handles: Vec<_> = (0..workers)
                .map(|_| {
                    let (todo, parsed, next, done, boxes) = (&todo, &parsed, &next, &done, &scope.boxes);
                    sc.spawn(move || -> Result<(Vec<Cand>, Vec<Cand>)> {
                        let mut wide = Vec::new();
                        let mut close = Vec::new();
                        loop {
                            let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            let Some(&(i, clip)) = todo.get(k) else { break };
                            if cancelled() {
                                return Err(Cancelled.into());
                            }
                            let idx = &self.indexes[i];
                            let q = build_query(&idx.f, parsed);
                            let searcher = idx.reader.searcher();
                            let b = if clip { Some(boxes.clone()) } else { None };
                            for (s, a) in searcher.search(&q, &weighted_collector(WIDE, near, 300.0, b.clone()))? {
                                if s > 0.0 {
                                    wide.push((s, i, a));
                                }
                            }
                            if near.is_some() {
                                for (s, a) in searcher.search(&q, &weighted_collector(CLOSE, near, 4.0, b))? {
                                    if s > 0.0 {
                                        close.push((s, i, a));
                                    }
                                }
                            }
                            let d = done.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                            progress(d, total);
                        }
                        Ok((wide, close))
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap_or_else(|_| Err(anyhow::anyhow!("search thread failed")))).collect()
        });
        let mut wide: Vec<Cand> = Vec::new();
        let mut close: Vec<Cand> = Vec::new();
        for r in results {
            let (w, c) = r?;
            wide.extend(w);
            close.extend(c);
        }
        if cancelled() {
            return Err(Cancelled.into());
        }
        wide.sort_by(|a, b| b.0.total_cmp(&a.0));
        close.sort_by(|a, b| b.0.total_cmp(&a.0));
        wide.truncate(WIDE);
        close.truncate(CLOSE);

        let searchers: Vec<_> = self.indexes.iter().map(|i| i.reader.searcher()).collect();
        let mut seen = std::collections::HashSet::new();
        let mut all: Vec<Hit> = Vec::new();
        for (s, i, a) in wide.into_iter().chain(close) {
            if !seen.insert((i, a)) {
                continue;
            }
            let doc: TantivyDocument = searchers[i].doc(a)?;
            let mut hit = rank::to_hit(&self.indexes[i].f, &doc, s);
            hit.src = i;
            all.push(hit);
        }

        // A house number that isn't in the address data ("123 N High St"
        // when only 100 and 125 are listed): estimate its position from the
        // neighboring numbers on the same street.
        if let Some(num) = parsed.house_number.clone() {
            let first = rank::rerank(all.clone(), &parsed, opts.near, 60);
            for street in first.iter().filter(|h| h.kind == "street").take(3) {
                if let Some(est) = self.estimate_address(street, &num)? {
                    all.push(est);
                }
            }
        }
        Ok(rank::rerank(all, &parsed, opts.near, limit))
    }

    fn estimate_address(&self, street: &Hit, num: &str) -> Result<Option<Hit>> {
        let Some(n) = leading_number(num) else { return Ok(None) };
        let street_norm = normalize::normalize(&street.name);
        let town = street.label.split(", ").next().unwrap_or("");
        let town_norm = normalize::normalize(town);
        let mut pts: Vec<(u64, f64, f64)> = Vec::new();
        // The street's own state holds its address points.
        if let Some(idx) = self.indexes.get(street.src) {
            let f = &idx.f;
            let mut must: Vec<(Occur, Box<dyn Query>)> = vec![(
                Occur::Must,
                Box::new(TermQuery::new(Term::from_field_text(f.kind, "address"), IndexRecordOption::Basic)),
            )];
            for t in street_norm.split(' ').chain(town_norm.split(' ')).filter(|t| !t.is_empty()) {
                must.push((
                    Occur::Must,
                    Box::new(TermQuery::new(Term::from_field_text(f.text, t), IndexRecordOption::Basic)),
                ));
            }
            let searcher = idx.reader.searcher();
            let top = searcher.search(&BooleanQuery::new(must), &TopDocs::with_limit(3000).order_by_score())?;
            for (_, addr) in top {
                let doc: TantivyDocument = searcher.doc(addr)?;
                let h = rank::to_hit(f, &doc, 0.0);
                let name_norm = normalize::normalize(&h.name);
                let rest = name_norm.split_once(' ').map(|x| x.1).unwrap_or("");
                let h_town = normalize::normalize(h.label.split(", ").next().unwrap_or(""));
                if rest != street_norm || h_town != town_norm {
                    continue;
                }
                if let Some(hn) = leading_number(&h.name) {
                    if hn == n {
                        return Ok(None); // the exact address exists; no estimate needed
                    }
                    pts.push((hn, h.lat, h.lon));
                }
            }
        }
        // Prefer neighbors on the same side of the street (same parity).
        let same: Vec<_> = pts.iter().copied().filter(|p| p.0 % 2 == n % 2).collect();
        let pool = if same.len() >= 2 { same } else { pts };
        let below = pool.iter().filter(|p| p.0 < n).max_by_key(|p| p.0).copied();
        let above = pool.iter().filter(|p| p.0 > n).min_by_key(|p| p.0).copied();
        let (lat, lon) = match (below, above) {
            (Some(a), Some(b)) => {
                let t = (n - a.0) as f64 / (b.0 - a.0) as f64;
                (a.1 + (b.1 - a.1) * t, a.2 + (b.2 - a.2) * t)
            }
            (Some(a), None) | (None, Some(a)) if a.0.abs_diff(n) <= 200 => (a.1, a.2),
            _ => return Ok(None),
        };
        let mut est = street.clone();
        est.id = format!("est:{num}:{}", street.id);
        est.kind = "address".into();
        est.name = format!("{num} {}", street.name);
        est.lat = lat;
        est.lon = lon;
        est.number = num.to_string();
        est.norm = format!("{} {}", normalize::normalize(num), street.norm);
        est.approximate = true;
        Ok(Some(est))
    }

    /// Nearest address, place or street to a point ("what's here").
    pub fn reverse(&self, lat: f64, lon: f64) -> Result<Option<Hit>> {
        self.reverse_in(lat, lon, None)
    }

    /// Like [`reverse`](Self::reverse), reading only the named indexes.
    pub fn reverse_in(&self, lat: f64, lon: f64, only: Option<&[String]>) -> Result<Option<Hit>> {
        let mut best: Option<(f64, Hit)> = None;
        let wanted: Vec<&OpenIndex> = self
            .indexes
            .iter()
            .filter(|i| only.is_none_or(|o| o.contains(&i.name)))
            .collect();
        for radius_deg in [0.0015, 0.005, 0.02] {
            for idx in &wanted {
                let q = bbox_query(&idx.f, lat, lon, radius_deg);
                let searcher = idx.reader.searcher();
                let top = searcher.search(&q, &TopDocs::with_limit(2000).order_by_score())?;
                for (_s, addr) in top {
                    let doc: TantivyDocument = searcher.doc(addr)?;
                    let mut hit = rank::to_hit(&idx.f, &doc, 0.0);
                    let d = rank::haversine_km(lat, lon, hit.lat, hit.lon);
                    // Prefer addresses and places over large areas at equal distance.
                    let penalty = match hit.kind.as_str() {
                        "address" | "place" => 0.0,
                        "street" => 0.02,
                        _ => 0.5,
                    };
                    hit.distance_km = Some(d);
                    let key = d + penalty;
                    if best.as_ref().map_or(true, |(b, _)| key < *b) {
                        best = Some((key, hit));
                    }
                }
            }
            if best.is_some() {
                break;
            }
        }
        Ok(best.map(|(_, h)| h))
    }
}

/// Top documents by text score x prominence x closeness to `near`.
fn weighted_collector(
    limit: usize,
    near: Option<(f64, f64)>,
    scale_km: f64,
    boxes: Option<Vec<[f64; 4]>>,
) -> impl tantivy::collector::Collector<Fruit = Vec<(f32, tantivy::DocAddress)>> {
    TopDocs::with_limit(limit).tweak_score(move |seg: &tantivy::SegmentReader| {
        let ff = seg.fast_fields();
        let imp = ff.f64("importance").map(|c| c.first_or_default_col(0.0)).ok();
        let lat = ff.f64("lat").map(|c| c.first_or_default_col(0.0)).ok();
        let lon = ff.f64("lon").map(|c| c.first_or_default_col(0.0)).ok();
        let boxes = boxes.clone();
        move |doc: tantivy::DocId, score: tantivy::Score| -> f32 {
            let (la, lo) = match (&lat, &lon) {
                (Some(a), Some(o)) => (a.get_val(doc), o.get_val(doc)),
                _ => (0.0, 0.0),
            };
            // Outside the chosen states: never a candidate.
            if let Some(b) = &boxes {
                if !in_boxes(b, la, lo) {
                    return -1.0;
                }
            }
            let i = imp.as_ref().map_or(0.3, |c| c.get_val(doc));
            let d = match near {
                Some((nlat, nlon)) if lat.is_some() => rank::haversine_km(nlat, nlon, la, lo),
                _ => 0.0,
            };
            score * (0.4 + i as f32) / (1.0 + (d / scale_km) as f32)
        }
    })
}

fn leading_number(s: &str) -> Option<u64> {
    let digits: String = s.trim().chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

fn bbox_query(f: &Fields, lat: f64, lon: f64, r: f64) -> Box<dyn Query> {
    let lat_q = RangeQuery::new(
        std::ops::Bound::Included(Term::from_field_f64(f.lat, lat - r)),
        std::ops::Bound::Included(Term::from_field_f64(f.lat, lat + r)),
    );
    let lon_q = RangeQuery::new(
        std::ops::Bound::Included(Term::from_field_f64(f.lon, lon - r)),
        std::ops::Bound::Included(Term::from_field_f64(f.lon, lon + r)),
    );
    Box::new(BooleanQuery::new(vec![
        (Occur::Must, Box::new(lat_q) as Box<dyn Query>),
        (Occur::Must, Box::new(lon_q)),
    ]))
}

fn build_query(f: &Fields, q: &rank::ParsedQuery) -> Box<dyn Query> {
    let mut clauses: Vec<(Occur, Box<dyn Query>)> = Vec::new();
    let n = q.tokens.len();
    for (i, tok) in q.tokens.iter().enumerate() {
        let last = i + 1 == n && !q.ends_with_space;
        let mut alts: Vec<(Occur, Box<dyn Query>)> = Vec::new();
        for field in [f.text, f.name_text] {
            let boost = if field == f.name_text { 1.5 } else { 1.0 };
            let term = Term::from_field_text(field, tok);
            alts.push((
                Occur::Should,
                Box::new(BoostQuery::new(
                    Box::new(TermQuery::new(term.clone(), IndexRecordOption::WithFreqs)),
                    2.0 * boost,
                )),
            ));
            let dist = rank::fuzzy_distance(tok);
            if last && tok.len() >= 2 {
                alts.push((
                    Occur::Should,
                    Box::new(BoostQuery::new(
                        Box::new(FuzzyTermQuery::new_prefix(term.clone(), 0, true)),
                        1.0 * boost,
                    )),
                ));
                if dist > 0 && tok.len() >= 5 {
                    alts.push((
                        Occur::Should,
                        Box::new(BoostQuery::new(
                            Box::new(FuzzyTermQuery::new_prefix(term.clone(), 1, true)),
                            0.4 * boost,
                        )),
                    ));
                }
            }
            if dist > 0 {
                alts.push((
                    Occur::Should,
                    Box::new(BoostQuery::new(
                        Box::new(FuzzyTermQuery::new(term, dist, true)),
                        0.6 * boost,
                    )),
                ));
            }
        }
        if normalize::is_house_number(tok) {
            alts.push((
                Occur::Should,
                Box::new(BoostQuery::new(
                    Box::new(TermQuery::new(Term::from_field_text(f.number, tok), IndexRecordOption::Basic)),
                    3.0,
                )),
            ));
            alts.push((
                Occur::Should,
                Box::new(TermQuery::new(Term::from_field_text(f.zip, tok), IndexRecordOption::Basic)),
            ));
        }
        for syn in normalize::category_synonyms(tok) {
            for w in syn.split(' ') {
                alts.push((
                    Occur::Should,
                    Box::new(BoostQuery::new(
                        Box::new(TermQuery::new(Term::from_field_text(f.text, w), IndexRecordOption::WithFreqs)),
                        0.8,
                    )),
                ));
            }
        }
        clauses.push((Occur::Should, Box::new(BooleanQuery::new(alts))));
    }
    Box::new(BooleanQuery::new(clauses))
}
