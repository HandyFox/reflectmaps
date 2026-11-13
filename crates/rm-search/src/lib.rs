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
        let limit = if opts.limit == 0 { 10 } else { opts.limit };
        if let Some(hit) = rank::parse_coordinates(query) {
            return Ok(vec![hit]);
        }
        let parsed = rank::ParsedQuery::new(query);
        if parsed.tokens.is_empty() {
            return Ok(vec![]);
        }
        let mut all: Vec<Hit> = Vec::new();
        for idx in &self.indexes {
            let q = build_query(&idx.f, &parsed);
            let searcher = idx.reader.searcher();
            let top = searcher.search(&q, &TopDocs::with_limit(400).order_by_score())?;
            for (bm25, addr) in top {
                let doc: TantivyDocument = searcher.doc(addr)?;
                let hit = rank::to_hit(&idx.f, &doc, bm25);
                all.push(hit);
            }
        }
        Ok(rank::rerank(all, &parsed, opts.near, limit))
    }

    /// Nearest address, place or street to a point ("what's here").
    pub fn reverse(&self, lat: f64, lon: f64) -> Result<Option<Hit>> {
        let mut best: Option<(f64, Hit)> = None;
        for radius_deg in [0.0015, 0.005, 0.02] {
            for idx in &self.indexes {
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
