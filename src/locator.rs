use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use crate::answer::{Answer, Counts, RegionCounts};
use crate::measurement::count_lines;

const LINES: &str = "lines";

// Where each number sits in a counter's own JSON, as the `read` block of an adapter declares it.
// Paths are read against the document as it stands, or, where the block names `each`, against
// every element that path matches, and then added up.
#[derive(Debug)]
pub struct Locator {
    each: Option<NamedPath>,
    // A path that has to match something, or the counter claims no such file. An `each` block
    // needs none: matching nothing already means that.
    claims: Option<NamedPath>,
    counts: BTreeMap<String, NamedPath>,
    regions: Option<RegionLocator>,
}

impl Locator {
    // Everything a read block is held to: its count paths are exactly `lines` and this dialect's
    // buckets, and every path parses.
    pub fn of(raw: RawLocator, buckets: &[String]) -> Result<Locator, String> {
        check_count_names(&raw.counts, buckets)?;
        let each = raw.each.map(|path| parse_elements_path("each", &path)).transpose()?;
        let claims = raw.claims.map(|path| parse_path("claims", &path)).transpose()?;
        if each.is_some() && claims.is_some() {
            return Err("names both each and claims, and an each that matches nothing already \
                        means the file is not claimed"
                .to_string());
        }
        let regions = match raw.regions {
            Some(raw) => {
                check_count_names(&raw.counts, buckets)?;
                Some(RegionLocator {
                    each: parse_elements_path("regions.each", &raw.each)?,
                    language: parse_count_path("regions.language", &raw.language)?,
                    counts: parse_count_paths(raw.counts)?,
                })
            }
            None => None,
        };
        Ok(Locator { each, claims, counts: parse_count_paths(raw.counts)?, regions })
    }

    pub fn read(&self, text: &str) -> Result<Option<Answer>, String> {
        let document: Value = serde_json::from_str(text)
            .map_err(|e| format!("what the counter printed does not parse: {e}"))?;
        // A key that is present but null is the counter saying nothing, not a claim.
        if let Some(claims) = &self.claims
            && walk(&document, &claims.steps).iter().all(|value| value.is_null())
        {
            return Ok(None);
        }
        let counts = match &self.each {
            Some(each) => {
                let elements = walk(&document, &each.steps);
                if elements.is_empty() {
                    return Ok(None);
                }
                let mut summed = BTreeMap::new();
                for (name, path) in &self.counts {
                    let mut total: u64 = 0;
                    for element in &elements {
                        total += read_number(element, path)?;
                    }
                    summed.insert(name.clone(), total);
                }
                summed
            }
            None => {
                let mut found = BTreeMap::new();
                for (name, path) in &self.counts {
                    found.insert(name.clone(), read_number(&document, path)?);
                }
                found
            }
        };
        let mut regions = Vec::new();
        if let Some(locator) = &self.regions {
            for element in walk(&document, &locator.each.steps) {
                let mut buckets = BTreeMap::new();
                for name in locator.counts.keys() {
                    if name != LINES {
                        buckets.insert(name.clone(), read_count(element, name, &locator.counts)?);
                    }
                }
                let language = read_text(element, &locator.language)?;
                regions.push(RegionCounts {
                    lines: read_count(element, LINES, &locator.counts)?,
                    language,
                    buckets,
                });
            }
            regions.sort();
        }
        let mut buckets = BTreeMap::new();
        let mut lines = 0;
        for (name, value) in counts {
            let converted = count_lines(value, &name)?;
            match name == LINES {
                true => lines = converted,
                false => {
                    buckets.insert(name, converted);
                }
            }
        }
        Ok(Some(Answer { counts: Counts { lines, buckets }, regions }))
    }
}

#[derive(Debug)]
struct RegionLocator {
    each: NamedPath,
    language: NamedPath,
    counts: BTreeMap<String, NamedPath>,
}

// The block as the adapter file writes it, with `lines` and the buckets sitting flat beside the
// fields that have a meaning of their own.
#[derive(Deserialize)]
pub struct RawLocator {
    each: Option<String>,
    claims: Option<String>,
    regions: Option<RawRegionLocator>,
    #[serde(flatten)]
    counts: BTreeMap<String, String>,
}

#[derive(Deserialize)]
pub struct RawRegionLocator {
    each: String,
    language: String,
    #[serde(flatten)]
    counts: BTreeMap<String, String>,
}

// The path kept beside its own text, so a refusal can say `total.extra` instead of describing it.
#[derive(Debug)]
struct NamedPath {
    shown: String,
    steps: Vec<Step>,
}

#[derive(Debug)]
enum Step {
    Key(String),
    Every,
}

fn check_count_names(counts: &BTreeMap<String, String>, buckets: &[String]) -> Result<(), String> {
    for name in counts.keys() {
        if name != LINES && !buckets.iter().any(|bucket| bucket == name) {
            return Err(format!(
                "gives a path for {name}, and this dialect counts {LINES}, {}",
                buckets.join(", ")
            ));
        }
    }
    for name in [LINES.to_string()].iter().chain(buckets) {
        if !counts.contains_key(name) {
            return Err(format!("gives no path for {name}"));
        }
    }
    Ok(())
}

fn parse_count_paths(
    raw: BTreeMap<String, String>,
) -> Result<BTreeMap<String, NamedPath>, String> {
    raw.into_iter().map(|(name, path)| Ok((name.clone(), parse_count_path(&name, &path)?))).collect()
}

fn parse_count_path(name: &str, text: &str) -> Result<NamedPath, String> {
    let path = parse_path(name, text)?;
    if path.steps.iter().any(|step| matches!(step, Step::Every)) {
        return Err(format!(
            "{name} = \"{text}\" fans out over [], and a count is one value per element"
        ));
    }
    Ok(path)
}

fn parse_elements_path(name: &str, text: &str) -> Result<NamedPath, String> {
    let path = parse_path(name, text)?;
    if !matches!(path.steps.last(), Some(Step::Every)) {
        return Err(format!(
            "{name} = \"{text}\" does not end in [], so it names one value instead of the \
             elements of a list"
        ));
    }
    Ok(path)
}

fn parse_path(name: &str, text: &str) -> Result<NamedPath, String> {
    let mut steps = Vec::new();
    for part in text.split('.') {
        let (key, every) = match part.strip_suffix("[]") {
            Some(key) => (key, true),
            None => (part, false),
        };
        if !key.is_empty() {
            steps.push(Step::Key(key.to_string()));
        } else if !every || part != "[]" {
            return Err(format!(
                "{name} = \"{text}\" is not a path: dot-separated names, each optionally ending \
                 in [], or [] alone for the document itself"
            ));
        }
        if every {
            steps.push(Step::Every);
        }
    }
    if steps.is_empty() {
        return Err(format!("{name} is an empty path, which points at nothing"));
    }
    Ok(NamedPath { shown: text.to_string(), steps })
}

fn walk<'a>(document: &'a Value, steps: &[Step]) -> Vec<&'a Value> {
    let mut found = vec![document];
    for step in steps {
        let mut next = Vec::new();
        for value in found {
            match step {
                Step::Key(name) => {
                    if let Some(inner) = value.get(name) {
                        next.push(inner);
                    }
                }
                Step::Every => {
                    if let Some(elements) = value.as_array() {
                        next.extend(elements.iter());
                    }
                }
            }
        }
        found = next;
    }
    found
}

fn read_count(
    base: &Value,
    name: &str,
    counts: &BTreeMap<String, NamedPath>,
) -> Result<u32, String> {
    let path = &counts[name];
    count_lines(read_number(base, path)?, &path.shown)
}

fn read_number(base: &Value, path: &NamedPath) -> Result<u64, String> {
    find_one(base, path)?
        .as_u64()
        .ok_or_else(|| format!("{} is not a whole number in what it printed", path.shown))
}

fn read_text(base: &Value, path: &NamedPath) -> Result<String, String> {
    find_one(base, path)?
        .as_str()
        .map(|text| text.to_string())
        .ok_or_else(|| format!("{} is not text in what it printed", path.shown))
}

fn find_one<'a>(base: &'a Value, path: &NamedPath) -> Result<&'a Value, String> {
    let found = walk(base, &path.steps);
    match found.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("nothing sits at {} in what it printed", path.shown)),
        many => Err(format!(
            "{} names {} values where one was wanted",
            path.shown,
            many.len()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEZURA: &str = include_str!("../tests/fixtures/output/mezura-nested.json");
    const MEZURA_REGION: &str = include_str!("../tests/fixtures/output/mezura-region.json");
    const SCC: &str = include_str!("../tests/fixtures/output/scc-nested.json");

    const MEZURA_READ: &str = "\
claims   = \"languages[]\"
lines    = \"total.lines\"
code     = \"total.code\"
comments = \"total.comments\"
extra    = \"total.extra\"

[regions]
each     = \"languages[].nested_languages[]\"
language = \"name\"
lines    = \"lines\"
code     = \"code\"
comments = \"comments\"
extra    = \"extra\"
";

    const SCC_READ: &str = "\
each     = \"[]\"
lines    = \"Lines\"
code     = \"Code\"
comments = \"Comment\"
blanks   = \"Blank\"
";

    const CLOC_READ: &str = "\
claims   = \"SUM\"
lines    = \"header.n_lines\"
code     = \"SUM.code\"
comments = \"SUM.comment\"
blanks   = \"SUM.blank\"
";

    #[test]
    fn an_absolute_block_reads_the_totals_and_its_regions_from_every_element() {
        let mezura = read_with(MEZURA_READ, &["code", "comments", "extra"], MEZURA).unwrap();
        assert_eq!(mezura.counts.lines, 13);
        assert_eq!(mezura.counts.buckets["code"], 9);
        assert_eq!(mezura.counts.buckets["comments"], 3);
        assert_eq!(mezura.counts.buckets["extra"], 1);
        let names: Vec<&str> = mezura.regions.iter().map(|r| r.language.as_str()).collect();
        assert_eq!(names, ["CSS", "JavaScript"]);
        assert_eq!(mezura.regions[1].lines, 3);
        assert_eq!(mezura.regions[1].buckets["extra"], 1);

        let with_blanks = MEZURA_READ.replace("extra", "blanks");
        let region = read_with(&with_blanks, &["code", "comments", "blanks"], MEZURA_REGION).unwrap();
        assert_eq!(region.counts.buckets["blanks"], 1);
        assert_eq!(region.regions[1].buckets["blanks"], 1);
    }

    #[test]
    fn an_each_block_sums_every_element_and_reports_no_regions() {
        let scc = read_with(SCC_READ, &["code", "comments", "blanks"], SCC).unwrap();
        assert_eq!(scc.counts.lines, 13);
        assert_eq!(scc.counts.buckets["code"], 13);
        assert_eq!(scc.counts.buckets["comments"], 0);
        assert_eq!(scc.counts.buckets["blanks"], 0);
        assert!(scc.regions.is_empty());
    }

    #[test]
    fn matching_nothing_claims_nothing_instead_of_answering_zero() {
        let empty = r#"{"total":{"lines":0,"code":0,"comments":0,"extra":0},"languages":[]}"#;
        assert!(read_with(MEZURA_READ, &["code", "comments", "extra"], empty).is_none());
        assert!(read_with(SCC_READ, &["code", "comments", "blanks"], "[]").is_none());
        assert!(read_with(CLOC_READ, &["code", "comments", "blanks"], "{}").is_none());
        let null = r#"{"SUM": null, "header": null}"#;
        assert!(read_with(CLOC_READ, &["code", "comments", "blanks"], null).is_none());
    }

    #[test]
    fn claims_may_name_one_value_and_a_document_holding_it_is_claimed() {
        let printed = r#"{"header": {"n_lines": 3}, "SUM": {"blank": 0, "comment": 1, "code": 2}}"#;
        let cloc = read_with(CLOC_READ, &["code", "comments", "blanks"], printed).unwrap();
        assert_eq!(cloc.counts.lines, 3);
        assert_eq!(cloc.counts.buckets["code"], 2);
        assert_eq!(cloc.counts.buckets["comments"], 1);
        assert_eq!(cloc.counts.buckets["blanks"], 0);
    }

    #[test]
    fn a_path_that_finds_nothing_is_an_error_once_the_file_is_claimed() {
        let locator = create(MEZURA_READ, &["code", "comments", "extra"]);
        let refused = locator.read(MEZURA_REGION).unwrap_err();
        assert!(refused.contains("nothing sits at total.extra"), "{refused}");
        assert!(locator.read("not json at all").unwrap_err().contains("does not parse"));

        let text = r#"[{"Lines": "13", "Code": 13, "Comment": 0, "Blank": 0}]"#;
        let refused = create(SCC_READ, &["code", "comments", "blanks"]).read(text).unwrap_err();
        assert!(refused.contains("Lines is not a whole number"), "{refused}");
    }

    #[test]
    fn the_count_paths_are_exactly_lines_and_the_buckets() {
        let missing = try_create(SCC_READ, &["code", "comments", "blanks", "documentation"]);
        assert!(missing.unwrap_err().contains("no path for documentation"));

        let stray = try_create(SCC_READ, &["code", "comments"]);
        let refused = stray.unwrap_err();
        assert!(refused.contains("gives a path for blanks"), "{refused}");
        assert!(refused.contains("lines, code, comments"), "{refused}");
    }

    #[test]
    fn a_count_may_not_fan_out_and_the_elements_paths_must() {
        let fanned = try_create(&SCC_READ.replace("\"Lines\"", "\"rows[].Lines\""),
                &["code", "comments", "blanks"]);
        assert!(fanned.unwrap_err().contains("fans out over []"));

        let single = try_create(&SCC_READ.replace("each     = \"[]\"",
                "each     = \"rows\""), &["code", "comments", "blanks"]);
        assert!(single.unwrap_err().contains("does not end in []"));

        let both = try_create(&SCC_READ.replace("each     = \"[]\"",
                "each     = \"[]\"\nclaims   = \"[]\""), &["code", "comments", "blanks"]);
        assert!(both.unwrap_err().contains("both each and claims"));

        let broken = try_create(&SCC_READ.replace("\"Lines\"", "\"a..b\""),
                &["code", "comments", "blanks"]);
        assert!(broken.unwrap_err().contains("is not a path"));
    }

    #[test]
    fn a_region_language_that_is_not_text_is_refused_by_its_path() {
        let text = r#"{"total": {"lines": 2, "code": 2, "comments": 0, "extra": 0},
            "languages": [{"nested_languages": [
                {"name": 3, "lines": 1, "code": 1, "comments": 0, "extra": 0}]}]}"#;
        let refused = create(MEZURA_READ, &["code", "comments", "extra"]).read(text).unwrap_err();
        assert!(refused.contains("name is not text"), "{refused}");
    }

    fn read_with(declaration: &str, buckets: &[&str], text: &str) -> Option<Answer> {
        create(declaration, buckets).read(text).unwrap()
    }

    fn create(declaration: &str, buckets: &[&str]) -> Locator {
        try_create(declaration, buckets).unwrap()
    }

    fn try_create(declaration: &str, buckets: &[&str]) -> Result<Locator, String> {
        let raw: RawLocator = toml::from_str(declaration).unwrap();
        let named: Vec<String> = buckets.iter().map(|bucket| bucket.to_string()).collect();
        Locator::of(raw, &named)
    }
}
