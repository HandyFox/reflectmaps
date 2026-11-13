//! Text normalization shared by indexing and querying.
//!
//! Both sides run the same function, so "123 N Main St" in the data and
//! "123 north main street" typed by a user end up as the same tokens.

use deunicode::deunicode;

/// Words that may appear in a query without carrying meaning on their own.
/// They are indexed (names contain them) but do not count against a result
/// when they fail to match.
pub const STOP_WORDS: &[&str] = &["the", "of", "in", "near", "at", "a", "an", "and", "by", "on", "to"];

/// US postal abbreviations mapped to the full word. Applied symmetrically.
const ABBREVIATIONS: &[(&str, &str)] = &[
    ("st", "street"),
    ("str", "street"),
    ("ave", "avenue"),
    ("av", "avenue"),
    ("avn", "avenue"),
    ("rd", "road"),
    ("dr", "drive"),
    ("blvd", "boulevard"),
    ("ln", "lane"),
    ("ct", "court"),
    ("pl", "place"),
    ("pkwy", "parkway"),
    ("pky", "parkway"),
    ("hwy", "highway"),
    ("cir", "circle"),
    ("ter", "terrace"),
    ("terr", "terrace"),
    ("trl", "trail"),
    ("sq", "square"),
    ("expy", "expressway"),
    ("fwy", "freeway"),
    ("tpke", "turnpike"),
    ("aly", "alley"),
    ("crk", "creek"),
    ("hts", "heights"),
    ("jct", "junction"),
    ("mtn", "mountain"),
    ("pt", "point"),
    ("rdg", "ridge"),
    ("spg", "spring"),
    ("spgs", "springs"),
    ("sta", "station"),
    ("vly", "valley"),
    ("vlg", "village"),
    ("xing", "crossing"),
    ("n", "north"),
    ("s", "south"),
    ("e", "east"),
    ("w", "west"),
    ("ne", "northeast"),
    ("nw", "northwest"),
    ("se", "southeast"),
    ("sw", "southwest"),
    ("mt", "mount"),
    ("ft", "fort"),
    ("ste", "suite"),
    ("apt", "apartment"),
    ("bldg", "building"),
    ("fl", "floor"),
    ("univ", "university"),
    ("ctr", "center"),
    ("cntr", "center"),
    ("centre", "center"),
    ("hosp", "hospital"),
    ("intl", "international"),
    ("natl", "national"),
    ("twp", "township"),
    ("sr", "state route"),
    ("cr", "county road"),
    ("co rd", "county road"),
    ("hwy", "highway"),
    ("i", "interstate"),
    ("&", "and"),
];

/// Lowercase, strip accents and punctuation, expand abbreviations.
/// Returns space-separated tokens.
pub fn normalize(input: &str) -> String {
    tokens(input).join(" ")
}

/// Same as [`normalize`] but returns the token list.
pub fn tokens(input: &str) -> Vec<String> {
    let ascii = deunicode(input).to_lowercase();
    // Treat apostrophes as joiners ("Joe's" -> "joes"), everything else
    // that is not alphanumeric as a separator. Keep "&" as its own token.
    let mut cleaned = String::with_capacity(ascii.len());
    for ch in ascii.chars() {
        if ch.is_ascii_alphanumeric() {
            cleaned.push(ch);
        } else if ch == '\'' || ch == '`' {
            // drop
        } else if ch == '&' {
            cleaned.push_str(" & ");
        } else {
            cleaned.push(' ');
        }
    }
    let raw: Vec<&str> = cleaned.split_whitespace().collect();
    let mut out: Vec<String> = Vec::with_capacity(raw.len() + 2);
    let mut i = 0;
    while i < raw.len() {
        // two-word abbreviations first
        if i + 1 < raw.len() {
            let pair = format!("{} {}", raw[i], raw[i + 1]);
            if let Some((_, full)) = ABBREVIATIONS.iter().find(|(a, _)| *a == pair) {
                out.extend(full.split(' ').map(str::to_string));
                i += 2;
                continue;
            }
        }
        let t = raw[i];
        if let Some((_, full)) = ABBREVIATIONS.iter().find(|(a, _)| *a == t) {
            out.extend(full.split(' ').map(str::to_string));
        } else {
            out.push(split_ordinal(t));
        }
        i += 1;
    }
    out
}

/// "1st" stays "1st"; "i71" -> "i71" is left alone. This hook exists so
/// ordinal handling can change in one place.
fn split_ordinal(t: &str) -> String {
    t.to_string()
}

/// Title-case text that arrived in ALL CAPS ("125 N HIGH ST" -> "125 N High St").
/// Mixed-case text is returned unchanged.
pub fn fix_case(s: &str) -> String {
    let letters = s.chars().filter(|c| c.is_alphabetic()).count();
    if letters < 3 || s.chars().any(|c| c.is_lowercase()) {
        return s.to_string();
    }
    const KEEP_UPPER: &[&str] = &["N", "S", "E", "W", "NE", "NW", "SE", "SW", "US", "SR", "CR", "II", "III", "IV", "PO"];
    s.split(' ')
        .map(|w| {
            if KEEP_UPPER.contains(&w) {
                return w.to_string();
            }
            let mut out = String::with_capacity(w.len());
            let mut start = true;
            for c in w.chars() {
                if start && c.is_alphabetic() {
                    out.extend(c.to_uppercase());
                    start = false;
                } else if c.is_alphabetic() {
                    out.extend(c.to_lowercase());
                } else {
                    out.push(c);
                    // "1ST" -> "1st": a digit does not start a new word
                    start = !c.is_ascii_digit() && c != '\'';
                }
            }
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn is_stop_word(t: &str) -> bool {
    STOP_WORDS.contains(&t)
}

/// True if the token is a house number such as "123", "123a" or "12-14".
pub fn is_house_number(t: &str) -> bool {
    let mut chars = t.chars();
    match chars.next() {
        Some(c) if c.is_ascii_digit() => {}
        _ => return false,
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    digits >= 1 && digits <= 6 && t.len() - digits <= 1
}

/// Turn a category key such as "gas_station" into searchable words.
pub fn category_words(key: &str) -> String {
    key.replace(['_', '-'], " ")
}

/// Extra words a category is known by in everyday US English.
pub fn category_synonyms(word: &str) -> &'static [&'static str] {
    match word {
        "gas" | "fuel" | "petrol" => &["gas station", "fuel"],
        "coffee" | "cafe" => &["coffee", "cafe"],
        "er" => &["emergency room", "hospital"],
        "grocery" | "groceries" | "supermarket" => &["grocery", "supermarket"],
        "pharmacy" | "drugstore" => &["pharmacy", "drugstore"],
        "hotel" | "motel" | "lodging" => &["hotel", "motel", "lodging"],
        "food" | "restaurant" | "restaurants" | "eat" => &["restaurant", "eatery", "food"],
        "atm" => &["atm", "bank"],
        "doctor" | "clinic" => &["doctor", "clinic", "medical"],
        "vet" => &["veterinarian", "vet"],
        "bar" | "pub" => &["bar", "pub"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_street_abbreviations() {
        assert_eq!(normalize("123 N. Main St."), "123 north main street");
        assert_eq!(normalize("123 north main street"), "123 north main street");
    }

    #[test]
    fn folds_accents_and_apostrophes() {
        assert_eq!(normalize("Café Joe's"), "cafe joes");
    }

    #[test]
    fn state_route() {
        assert_eq!(normalize("SR 4"), normalize("State Route 4"));
        assert_eq!(normalize("Co Rd 5"), normalize("County Road 5"));
    }

    #[test]
    fn title_cases_all_caps() {
        assert_eq!(fix_case("125 N HIGH ST"), "125 N High St");
        assert_eq!(fix_case("100 W 1ST AVE"), "100 W 1st Ave");
        assert_eq!(fix_case("McDonald's"), "McDonald's");
    }

    #[test]
    fn house_numbers() {
        assert!(is_house_number("123"));
        assert!(is_house_number("123a"));
        assert!(!is_house_number("main"));
        assert!(!is_house_number("1st1"));
    }
}
