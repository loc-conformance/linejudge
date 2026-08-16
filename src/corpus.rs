use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::buckets::{check_buckets, find_buckets};

const CASE_FILE: &str = "case.toml";
const INPUT_STEM: &str = "input.";

pub struct Corpus {
    pub cases: Vec<Case>,
}

impl Corpus {
    pub fn read(dir: &Path) -> Result<Corpus, Vec<Fault>> {
        let mut dirs = match fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect::<Vec<_>>(),
            Err(error) => {
                return Err(vec![Fault {
                    case: dir.display().to_string(),
                    message: format!("the corpus directory could not be opened: {error}"),
                }]);
            }
        };
        dirs.sort();

        let mut cases = Vec::with_capacity(dirs.len());
        let mut faults = Vec::new();
        for path in dirs {
            match Case::read(&path) {
                Ok(case) => cases.push(case),
                Err(mut found) => faults.append(&mut found),
            }
        }
        if faults.is_empty() {
            Ok(Corpus { cases })
        } else {
            Err(faults)
        }
    }
}

pub struct Case {
    pub name: String,
    pub number: String,
    pub input: PathBuf,
    pub lines: u32,
    pub trap: String,
    pub declares_regions: bool,
    pub required_regions: Vec<RegionExtent>,
    pub optional_regions: Vec<RegionExtent>,
    pub answers: Vec<Answer>,
}

impl Case {
    pub fn read(dir: &Path) -> Result<Case, Vec<Fault>> {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| dir.display().to_string());
        let one = |message: String| vec![Fault { case: name.clone(), message }];

        let input = match find_input(dir) {
            Ok(path) => path,
            Err(message) => return Err(one(message)),
        };
        let text = match fs::read_to_string(&input) {
            Ok(text) => text,
            Err(error) => return Err(one(format!("the input could not be read: {error}"))),
        };
        let lines = text.lines().count() as u32;

        let declaration = match fs::read_to_string(dir.join(CASE_FILE)) {
            Ok(text) => text,
            Err(error) => return Err(one(format!("{CASE_FILE} could not be read: {error}"))),
        };
        let raw: RawCase = match toml::from_str(&declaration) {
            Ok(raw) => raw,
            Err(error) => return Err(one(format!("{CASE_FILE} does not parse: {error}"))),
        };

        let declares_regions = raw.region.is_some();
        let regions = raw.region.unwrap_or_default();
        let required_regions: Vec<RegionExtent> =
            regions.required.into_iter().map(RegionExtent::from).collect();
        let optional_regions: Vec<RegionExtent> =
            regions.optional.into_iter().map(RegionExtent::from).collect();

        let mut answers = Vec::new();
        let mut faults = Vec::new();
        for (counter, dialects) in raw.answer {
            for (dialect, raw) in dialects {
                match Answer::of(counter.clone(), dialect, raw) {
                    Ok(answer) => answers.push(answer),
                    Err(message) => faults.push(Fault { case: name.clone(), message }),
                }
            }
        }
        answers.sort_by(|a, b| (&a.name_of_counter, &a.dialect).cmp(&(&b.name_of_counter, &b.dialect)));

        let number = name.split('-').next().unwrap_or(&name).to_string();
        let case = Case {
            name,
            number,
            input,
            lines,
            trap: raw.trap,
            declares_regions,
            required_regions,
            optional_regions,
            answers,
        };
        faults.extend(case.find_faults());
        if faults.is_empty() { Ok(case) } else { Err(faults) }
    }

    pub fn find_answer(&self, counter: &str, dialect: &str) -> Option<&Answer> {
        self.answers.iter().find(|a| a.name_of_counter == counter && a.dialect == dialect)
    }

    fn find_faults(&self) -> Vec<Fault> {
        let mut faults = Vec::new();
        let mut fault = |message: String| faults.push(Fault { case: self.name.clone(), message });

        if self.trap.trim().is_empty() {
            fault("the trap says nothing".to_string());
        }
        for answer in &self.answers {
            let key = format!("{}.{}", answer.name_of_counter, answer.dialect);
            let Some(buckets) = find_buckets(&answer.name_of_counter, &answer.dialect) else {
                fault(format!("{key} is a block of no counter this suite knows"));
                continue;
            };
            let Some(given) = &answer.given else { continue };

            for (side, counts) in [("real", &given.real), ("counted", &given.counted)] {
                if let Err(wrong) = check_buckets(&counts.buckets, &buckets) {
                    fault(format!("{key}.{side} {wrong}"));
                }
            }
            for region in given.real_regions.iter().chain(&given.counted_regions) {
                if let Err(wrong) = check_buckets(&region.buckets, &buckets) {
                    fault(format!("{key} region {} {wrong}", region.language));
                }
            }
            // No counter reports one language twice for one file, so a repeated language is a
            // copy of a line rather than an answer, and it would fail the case for ever.
            for (side, regions) in
                [("real", &given.real_regions), ("counted", &given.counted_regions)]
            {
                for pair in regions.windows(2) {
                    if pair[0].language == pair[1].language {
                        fault(format!("{key}.{side} names {} twice", pair[0].language));
                    }
                }
            }

            // Only the real side is held to the file and to its own arithmetic. A counter that
            // reports 18 lines for a file of 17, or buckets that do not add up, has to have
            // somewhere to be written down, and case 5400 is exactly that.
            if given.real.lines != self.lines {
                fault(format!(
                    "{key}.real says {} lines and the file has {}",
                    given.real.lines, self.lines
                ));
            }
            if given.real.sum() != u64::from(given.real.lines) {
                fault(format!("{key}.real does not add up to its own line count"));
            }
            for region in &given.real_regions {
                if region.sum() != u64::from(region.lines) {
                    fault(format!(
                        "{key}.real region {} does not add up to its own line count",
                        region.language
                    ));
                }
            }

            if self.declares_regions != given.states_regions {
                let said = if given.states_regions { "states" } else { "leaves out" };
                let declared = if self.declares_regions { "does" } else { "does not" };
                fault(format!(
                    "{key} {said} its regions and the case {declared} declare any"
                ));
            }
            // A required region of no lines is answered by naming it nowhere: no counter prints a
            // row for a region with nothing in it.
            for extent in &self.required_regions {
                let found = given
                    .real_regions
                    .iter()
                    .find(|region| region.language == extent.language);
                match (extent.lines, found) {
                    (0, Some(region)) => fault(format!(
                        "{key}.real gives {} lines to {}, which the case declares empty",
                        region.lines, extent.language
                    )),
                    (0, None) => {}
                    (_, None) => fault(format!(
                        "{key}.real misses the required region {} of {} lines",
                        extent.language, extent.lines
                    )),
                    (wanted, Some(region)) if region.lines != wanted => fault(format!(
                        "{key}.real gives {} lines to the required region {}, which the case says is {}",
                        region.lines, extent.language, wanted
                    )),
                    _ => {}
                }
            }
            for region in &given.real_regions {
                let declared = self
                    .required_regions
                    .iter()
                    .chain(&self.optional_regions)
                    .any(|e| e.language == region.language && e.lines == region.lines);
                if !declared {
                    fault(format!(
                        "{key}.real reports {} of {} lines, which is in neither region list",
                        region.language, region.lines
                    ));
                }
            }

            match (given.agrees(), given.says_something()) {
                (true, true) => fault(format!("{key} carries a note and answers correctly")),
                (false, false) => fault(format!("{key} answers differently and says nothing")),
                _ => {}
            }
        }
        faults
    }
}

pub struct RegionExtent {
    pub language: String,
    pub lines: u32,
}

impl From<RawExtent> for RegionExtent {
    fn from(raw: RawExtent) -> RegionExtent {
        RegionExtent { language: raw.language, lines: raw.lines }
    }
}

pub struct Answer {
    pub name_of_counter: String,
    pub dialect: String,
    /// `None` is a counter that does not claim the file, which is not the same as answering zero
    /// and is not a failure of any kind.
    pub given: Option<Recorded>,
}

impl Answer {
    fn of(name_of_counter: String, dialect: String, raw: RawAnswer) -> Result<Answer, String> {
        let key = format!("{name_of_counter}.{dialect}");
        if raw.unclaimed {
            let alone = raw.real.is_none()
                && raw.counted.is_none()
                && raw.note.is_none()
                && raw.real_regions.is_none()
                && raw.counted_regions.is_none();
            if !alone {
                return Err(format!("{key} is unclaimed and still answers"));
            }
            return Ok(Answer { name_of_counter, dialect, given: None });
        }
        let (Some(real), Some(counted)) = (raw.real, raw.counted) else {
            return Err(format!("{key} is missing its real or its counted answer"));
        };
        let states_regions = raw.real_regions.is_some() && raw.counted_regions.is_some();
        if raw.real_regions.is_some() != raw.counted_regions.is_some() {
            return Err(format!("{key} states one of its two region lists and not the other"));
        }
        Ok(Answer {
            name_of_counter,
            dialect,
            given: Some(Recorded {
                real: real.into(),
                real_regions: regions_of(raw.real_regions),
                counted: counted.into(),
                counted_regions: regions_of(raw.counted_regions),
                note: raw.note,
                states_regions,
            }),
        })
    }
}

/// What a case file records about one counter: the right answer under that counter's own
/// definitions, what it printed when it was last measured, and the note saying why they differ.
pub struct Recorded {
    pub real: Counts,
    pub real_regions: Vec<RegionCounts>,
    pub counted: Counts,
    pub counted_regions: Vec<RegionCounts>,
    pub note: Option<String>,
    pub states_regions: bool,
}

impl Recorded {
    pub fn agrees(&self) -> bool {
        self.real == self.counted && self.real_regions == self.counted_regions
    }

    pub fn says_something(&self) -> bool {
        self.note.as_ref().is_some_and(|note| !note.trim().is_empty())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct Counts {
    pub lines: u32,
    pub buckets: BTreeMap<String, u32>,
}

impl Counts {
    // Summed wide, because a case file is text and can hold any number that fits its own field.
    fn sum(&self) -> u64 {
        self.buckets.values().map(|value| u64::from(*value)).sum()
    }
}

impl From<RawCounts> for Counts {
    fn from(raw: RawCounts) -> Counts {
        Counts { lines: raw.lines, buckets: raw.buckets }
    }
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegionCounts {
    pub language: String,
    pub lines: u32,
    pub buckets: BTreeMap<String, u32>,
}

impl RegionCounts {
    fn sum(&self) -> u64 {
        self.buckets.values().map(|value| u64::from(*value)).sum()
    }
}

impl From<RawRegionCounts> for RegionCounts {
    fn from(raw: RawRegionCounts) -> RegionCounts {
        RegionCounts { language: raw.language, lines: raw.lines, buckets: raw.buckets }
    }
}

#[derive(Debug)]
pub struct Fault {
    pub case: String,
    pub message: String,
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.case, self.message)
    }
}

fn find_input(dir: &Path) -> Result<PathBuf, String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("the case could not be opened: {e}"))?;
    let mut found: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().starts_with(INPUT_STEM))
                .unwrap_or(false)
        })
        .collect();
    found.sort();
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err(format!("no {INPUT_STEM}<extension> file")),
        n => Err(format!("{n} files named {INPUT_STEM}<extension>, and a case is one file")),
    }
}

fn regions_of(raw: Option<Vec<RawRegionCounts>>) -> Vec<RegionCounts> {
    let mut regions: Vec<RegionCounts> =
        raw.unwrap_or_default().into_iter().map(RegionCounts::from).collect();
    regions.sort();
    regions
}

// These match a case file key for key. The types at the top of the file match a case that has been
// read and checked. Two shapes because the file is allowed to say less: a block that says
// `unclaimed` has no counts under it, and a case with one language in it has no region lists.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCase {
    trap: String,
    region: Option<RawRegions>,
    answer: BTreeMap<String, BTreeMap<String, RawAnswer>>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRegions {
    #[serde(default)]
    required: Vec<RawExtent>,
    #[serde(default)]
    optional: Vec<RawExtent>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExtent {
    language: String,
    lines: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAnswer {
    #[serde(default)]
    unclaimed: bool,
    real: Option<RawCounts>,
    #[serde(rename = "real-regions")]
    real_regions: Option<Vec<RawRegionCounts>>,
    counted: Option<RawCounts>,
    #[serde(rename = "counted-regions")]
    counted_regions: Option<Vec<RawRegionCounts>>,
    note: Option<String>,
}

// serde refuses `deny_unknown_fields` beside a flattened map, so a misspelled bucket is not caught
// here. What catches it is the check that a block's buckets are exactly its counter's.
#[derive(Deserialize)]
struct RawCounts {
    lines: u32,
    #[serde(flatten)]
    buckets: BTreeMap<String, u32>,
}

#[derive(Deserialize)]
struct RawRegionCounts {
    language: String,
    lines: u32,
    #[serde(flatten)]
    buckets: BTreeMap<String, u32>,
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    const ONE_CASE: &str = r#"
trap = """
a line comment inside a block comment is part of the block"""

[answer.tokei.default]
real    = { lines = 2, code = 1, comments = 1, blanks = 0 }
counted = { lines = 2, code = 1, comments = 1, blanks = 0 }
"#;

    #[test]
    fn every_case_of_the_corpus_is_read_without_a_fault() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("cases");
        match Corpus::read(&dir) {
            Ok(corpus) => assert_eq!(corpus.cases.len(), 79),
            Err(faults) => {
                let report: Vec<String> = faults.iter().map(|f| f.to_string()).collect();
                panic!("{}", report.join("\n"));
            }
        }
    }

    #[test]
    fn every_case_of_the_corpus_answers_every_counter_this_suite_knows() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("cases");
        let corpus = Corpus::read(&dir).unwrap_or_else(|_| panic!("the corpus does not read"));
        let mut silent = Vec::new();
        for case in &corpus.cases {
            for (counter, dialect) in crate::buckets::find_every_dialect() {
                if case.find_answer(counter, dialect).is_none() {
                    silent.push(format!("{} says nothing about {counter}.{dialect}", case.name));
                }
            }
        }
        assert!(silent.is_empty(), "{}", silent.join("\n"));
    }

    #[test]
    fn a_note_beside_answers_that_agree_is_refused() {
        let case = ONE_CASE.to_string() + "note    = \"\"\"\nsomething\"\"\"\n";
        let faults = read_a_broken_case("a_note_beside_answers_that_agree", &case);
        assert!(faults.iter().any(|f| f.message.contains("carries a note")), "{faults:?}");
    }

    #[test]
    fn a_missing_note_where_the_answers_differ_is_refused() {
        let case = ONE_CASE.replace("counted = { lines = 2, code = 1", "counted = { lines = 2, code = 2");
        let case = case.replace("comments = 1, blanks = 0 }\n", "comments = 0, blanks = 0 }\n");
        let faults = read_a_broken_case("a_missing_note", &case);
        assert!(faults.iter().any(|f| f.message.contains("says nothing")), "{faults:?}");
    }

    #[test]
    fn a_real_answer_that_is_not_the_whole_file_is_refused() {
        let case = ONE_CASE.replace("real    = { lines = 2", "real    = { lines = 3");
        let faults = read_a_broken_case("a_real_answer_that_is_not_the_file", &case);
        assert!(faults.iter().any(|f| f.message.contains("and the file has 2")), "{faults:?}");
        assert!(faults.iter().any(|f| f.message.contains("does not add up")), "{faults:?}");
    }

    #[test]
    fn a_bucket_the_counter_has_not_is_refused() {
        let case = ONE_CASE.replace("blanks = 0 }", "extra = 0 }");
        let faults = read_a_broken_case("a_bucket_the_counter_has_not", &case);
        assert!(faults.iter().any(|f| f.message.contains("has no blanks bucket")), "{faults:?}");
    }

    #[test]
    fn a_block_of_an_unknown_counter_is_refused() {
        let case = ONE_CASE.replace("[answer.tokei.default]", "[answer.tokei.strict]");
        let faults = read_a_broken_case("a_block_of_an_unknown_counter", &case);
        assert!(faults.iter().any(|f| f.message.contains("no counter this suite knows")), "{faults:?}");
    }

    #[test]
    fn a_region_in_neither_list_is_refused() {
        let case = ONE_CASE.replace(
            "[answer.tokei.default]",
            "region.required = []\n\n[answer.tokei.default]",
        ) + "real-regions    = [{ language = \"Perl\", lines = 1, code = 1, comments = 0, blanks = 0 }]\n\
             counted-regions = []\n\
             note            = \"\"\"\nsomething\"\"\"\n";
        let faults = read_a_broken_case("a_region_in_neither_list", &case);
        assert!(faults.iter().any(|f| f.message.contains("in neither region list")), "{faults:?}");
    }

    #[test]
    fn an_empty_required_region_is_answered_by_silence() {
        let quiet = ONE_CASE.replace(
            "[answer.tokei.default]",
            "region.required = [{ language = \"JavaScript\", lines = 0 }]\n\n[answer.tokei.default]",
        ) + "real-regions    = []\ncounted-regions = []\n";
        assert!(read_the_case("an_empty_region_answered_by_silence", &quiet).is_ok());

        let loud = quiet.replace(
            "real-regions    = []",
            "real-regions    = [{ language = \"JavaScript\", lines = 1, code = 1, comments = 0, blanks = 0 }]",
        );
        let faults = read_a_broken_case("an_empty_region_given_lines", &loud);
        assert!(faults.iter().any(|f| f.message.contains("declares empty")), "{faults:?}");
    }

    #[test]
    fn region_lists_belong_to_a_case_that_declares_regions() {
        let case = ONE_CASE.to_string() + "real-regions    = []\ncounted-regions = []\n";
        let faults = read_a_broken_case("region_lists_without_a_declaration", &case);
        assert!(faults.iter().any(|f| f.message.contains("does not declare any")), "{faults:?}");
    }

    fn read_the_case(name: &str, declaration: &str) -> Result<Corpus, Vec<Fault>> {
        let root = env::temp_dir().join(format!("linejudge-{name}"));
        let dir = root.join("0400-a_case_built_by_a_test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("input.c"), "/* a block\n*/ int x = 1;\n").unwrap();
        fs::write(dir.join(CASE_FILE), declaration).unwrap();
        let read = Corpus::read(&root);
        fs::remove_dir_all(&root).unwrap();
        read
    }

    fn read_a_broken_case(name: &str, declaration: &str) -> Vec<Fault> {
        match read_the_case(name, declaration) {
            Ok(_) => panic!("the case was read without a fault"),
            Err(faults) => faults,
        }
    }

}
