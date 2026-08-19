use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;

use serde::Deserialize;

use crate::adapter::UNKNOWN_VERSION;
use crate::answer::{Answer, Counts, RegionCounts};
use crate::dialects::{Dialects, check_buckets};

pub const RECORDED_DIR: &str = "recorded";
const RECORDED_EXTENSION: &str = "toml";

/// The photograph of one counter: what it printed for each case on the day it was measured, at the
/// version written at the top. A counter this suite holds no such file for is measured all the
/// same; the file only adds drift, the question of whether the counter still does what it did.
pub struct RecordedAnswers {
    pub counter: String,
    pub version: String,
    answers: BTreeMap<(String, String), RecordedAnswer>,
    exceptions: BTreeMap<(String, String), Exception>,
}

impl RecordedAnswers {
    /// A file that is not there is a counter nobody here has photographed, which is the ordinary
    /// state of anybody else's counter and not an error.
    /// Layered like the adapters and the dialects: the last directory holding a file for this
    /// counter is the one read, and a counter no directory names has no record, which is the
    /// ordinary state for anybody's tool but the ones measured here.
    pub fn read(
        dirs: &[PathBuf],
        counter: &str,
        dialects: &Dialects,
    ) -> Result<Option<RecordedAnswers>, Vec<String>> {
        let named = format!("{counter}.{RECORDED_EXTENSION}");
        let Some(path) = dirs.iter().rev().map(|dir| dir.join(&named)).find(|path| path.is_file())
        else {
            return Ok(None);
        };
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(vec![format!("{} could not be read: {error}", path.display())]);
            }
        };
        let raw: RawRecorded = toml::from_str(&text)
            .map_err(|e| vec![format!("{} does not parse: {e}", path.display())])?;
        let where_it_is = path.display();
        let mut faults = Vec::new();
        if raw.counter != counter {
            faults.push(format!(
                "{where_it_is} says it records {}, and a counter's answers are the file named \
                 after it",
                raw.counter
            ));
        }
        if raw.version.trim().is_empty() {
            faults.push(format!(
                "{where_it_is} names no version, so nothing says which build these answers came \
                 from"
            ));
        }

        let mut answers = BTreeMap::new();
        for (case, blocks) in raw.answer {
            for (dialect, block) in blocks {
                let key = format!("answer.{case}.{dialect}");
                let buckets = match find_buckets(dialects, counter, &dialect) {
                    Ok(buckets) => buckets,
                    Err(message) => {
                        faults.push(format!("{where_it_is}: {key} {message}"));
                        continue;
                    }
                };
                match RecordedAnswer::of(block, buckets) {
                    Ok(answer) => {
                        answers.insert((case.clone(), dialect), answer);
                    }
                    Err(found) => faults
                        .extend(found.into_iter().map(|m| format!("{where_it_is}: {key} {m}"))),
                }
            }
        }
        let mut exceptions = BTreeMap::new();
        for (case, blocks) in raw.exception {
            for (dialect, block) in blocks {
                let key = format!("exception.{case}.{dialect}");
                let buckets = match find_buckets(dialects, counter, &dialect) {
                    Ok(buckets) => buckets,
                    Err(message) => {
                        faults.push(format!("{where_it_is}: {key} {message}"));
                        continue;
                    }
                };
                match Exception::of(block, buckets) {
                    Ok(exception) => {
                        exceptions.insert((case.clone(), dialect), exception);
                    }
                    Err(found) => faults
                        .extend(found.into_iter().map(|m| format!("{where_it_is}: {key} {m}"))),
                }
            }
        }
        if !faults.is_empty() {
            return Err(faults);
        }
        Ok(Some(RecordedAnswers { counter: raw.counter, version: raw.version, answers, exceptions }))
    }

    pub fn find(&self, case: &str, dialect: &str) -> Option<&RecordedAnswer> {
        self.answers.get(&(case.to_string(), dialect.to_string()))
    }

    pub fn find_exception(&self, case: &str, dialect: &str) -> Option<&Exception> {
        self.exceptions.get(&(case.to_string(), dialect.to_string()))
    }

    /// Every case this file speaks about, answers and exceptions together, for the report line
    /// that says an entry names no case of the corpus.
    pub fn cases_spoken_about(&self) -> impl Iterator<Item = (&str, &str)> {
        self.answers
            .keys()
            .chain(self.exceptions.keys())
            .map(|(case, dialect)| (case.as_str(), dialect.as_str()))
    }
}

/// What one counter printed for one case, in one of its ways of counting. `counted` is `None`
/// where it claimed no such file. The flag says out loud whether what it printed differs from
/// what its own rules ask for, so a flag the numbers contradict is refused instead of read.
pub struct RecordedAnswer {
    pub counted: Option<Answer>,
    pub is_known_failure: bool,
    pub note: Option<String>,
}

impl RecordedAnswer {
    fn of(raw: RawAnswer, buckets: &[String]) -> Result<RecordedAnswer, Vec<String>> {
        let note = raw.note.filter(|note| !note.trim().is_empty());
        if raw.unclaimed {
            let mut faults = Vec::new();
            if raw.counted.is_some() || raw.counted_regions.is_some() {
                faults.push("claims no such file and still answers".to_string());
            }
            if raw.is_known_failure {
                faults.push(
                    "claims no such file and calls it a known failure, and not claiming a file \
                     is an answer of its own, never a failure"
                        .to_string(),
                );
            }
            if !faults.is_empty() {
                return Err(faults);
            }
            return Ok(RecordedAnswer { counted: None, is_known_failure: false, note });
        }
        let Some(counted) = raw.counted else {
            return Err(vec![
                "writes down no answer and does not say it claims no such file".to_string(),
            ]);
        };
        let counted = Answer {
            counts: Counts { lines: counted.lines, buckets: counted.buckets },
            regions: collect_regions(raw.counted_regions),
        };
        let faults = check_the_shape_of(&counted, buckets);
        if !faults.is_empty() {
            return Err(faults);
        }
        Ok(RecordedAnswer {
            counted: Some(counted),
            is_known_failure: raw.is_known_failure,
            note,
        })
    }
}

/// A (case, way of counting) whose deliberate behavior no rule over spans can express. It replaces
/// the answer the rules would derive, the case then passes and is counted apart, and the note is
/// mandatory: an exception is a claim about the tool's own intent, and a claim with no reason is
/// worth nothing.
pub struct Exception {
    pub expected: Answer,
    pub note: String,
}

impl Exception {
    fn of(raw: RawException, buckets: &[String]) -> Result<Exception, Vec<String>> {
        let expected = Answer {
            counts: Counts { lines: raw.expected.lines, buckets: raw.expected.buckets },
            regions: collect_regions(raw.expected_regions),
        };
        let mut faults = check_the_shape_of(&expected, buckets);
        if raw.note.trim().is_empty() {
            faults.push("carries no note, and an exception claims intent".to_string());
        }
        if !faults.is_empty() {
            return Err(faults);
        }
        Ok(Exception { expected, note: raw.note })
    }
}

/// Drift is only judged between two runs of the same build, and an unknown version on either side
/// is not the same build as anything, itself included.
pub fn is_same_build(recorded: &str, running: &str) -> bool {
    recorded != UNKNOWN_VERSION && running != UNKNOWN_VERSION && recorded == running
}

fn find_buckets<'a>(
    dialects: &'a Dialects,
    counter: &str,
    dialect: &str,
) -> Result<&'a [String], String> {
    match dialects.find(counter, dialect) {
        Some(found) => Ok(&found.buckets),
        None => Err(format!("is a block of no way of counting {counter} has")),
    }
}

fn check_the_shape_of(answer: &Answer, buckets: &[String]) -> Vec<String> {
    let mut faults = Vec::new();
    if let Err(wrong) = check_buckets(&answer.counts.buckets, buckets) {
        faults.push(wrong);
    }
    for region in &answer.regions {
        if let Err(wrong) = check_buckets(&region.buckets, buckets) {
            faults.push(format!("region {} {wrong}", region.language));
        }
    }
    for pair in answer.regions.windows(2) {
        if pair[0].language == pair[1].language {
            faults.push(format!("names {} twice", pair[0].language));
        }
    }
    faults
}

fn collect_regions(raw: Option<Vec<RawRegionCounts>>) -> Vec<RegionCounts> {
    let mut regions: Vec<RegionCounts> = raw
        .unwrap_or_default()
        .into_iter()
        .map(|raw| RegionCounts { language: raw.language, lines: raw.lines, buckets: raw.buckets })
        .collect();
    regions.sort();
    regions
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRecorded {
    counter: String,
    version: String,
    #[serde(default)]
    answer: BTreeMap<String, BTreeMap<String, RawAnswer>>,
    #[serde(default)]
    exception: BTreeMap<String, BTreeMap<String, RawException>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAnswer {
    #[serde(default)]
    unclaimed: bool,
    #[serde(rename = "is-known-failure", default)]
    is_known_failure: bool,
    counted: Option<RawCounts>,
    #[serde(rename = "counted-regions")]
    counted_regions: Option<Vec<RawRegionCounts>>,
    note: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawException {
    expected: RawCounts,
    #[serde(rename = "expected-regions")]
    expected_regions: Option<Vec<RawRegionCounts>>,
    note: String,
}

// serde refuses `deny_unknown_fields` beside a flattened map, so a misspelled bucket is not caught
// here. What catches it is the check that a block's buckets are exactly its dialect's.
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
    use std::path::Path;
    use std::slice;

    use crate::corpus::Corpus;
    use crate::deriver::derive_answer;
    use crate::dialects::read_the_shipped_dialects;

    use super::*;

    const ONE_FILE: &str = r#"
counter = "tokei"
version = "tokei 14.0.0"

[answer.0400-a_case.default]
counted = { lines = 2, code = 1, comments = 1, blanks = 0 }

[answer.0500-a_failing_case.default]
is-known-failure = true
counted = { lines = 2, code = 2, comments = 0, blanks = 0 }
note = """
the line comment is swallowed by the block above it"""
"#;

    // The roster is the three counters this suite itself declares, and completeness of their
    // records is checked here rather than at run time, since for anybody else's counter a missing
    // record is the ordinary state. The flags are re-checked at run time all the same, because a
    // dialect rule can change after this test last ran on the machine that edited it.
    #[test]
    fn every_roster_counter_records_every_case_and_nothing_contradicts_the_rules() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let dialects = read_the_shipped_dialects();
        let corpus =
            Corpus::read(&root.join("cases")).unwrap_or_else(|faults| panic!("{faults:?}"));
        let mut wrong = Vec::new();
        for counter in ["mezura", "scc", "tokei"] {
            let record = RecordedAnswers::read(&[root.join(RECORDED_DIR)], counter, &dialects)
                .unwrap_or_else(|faults| panic!("{}", faults.join("\n")))
                .unwrap_or_else(|| panic!("{counter} has no recorded answers"));
            let text = fs::read_to_string(root.join(RECORDED_DIR).join(format!("{counter}.toml")))
                .unwrap();
            let raw: RawRecorded = toml::from_str(&text).unwrap();
            for dialect in dialects.iter().filter(|d| d.counter == counter) {
                for case in &corpus.cases {
                    let key = format!("{counter}.{}: {}", dialect.name, case.name);
                    let Some(entry) = record.find(&case.name, &dialect.name) else {
                        wrong.push(format!("{key} has no recorded answer"));
                        continue;
                    };
                    let real = match record.find_exception(&case.name, &dialect.name) {
                        Some(exception) => exception.expected.clone(),
                        None => derive_answer(&case.truth, dialect, &corpus.readings)
                            .unwrap_or_else(|faults| panic!("{key}: {faults:?}"))
                            .real,
                    };
                    let Some(counted) = &entry.counted else { continue };
                    if entry.is_known_failure == (*counted == real) {
                        wrong.push(format!("{key}: the flag contradicts the numbers beside it"));
                    }
                    let regions_written = raw.answer[&case.name][&dialect.name]
                        .counted_regions
                        .is_some();
                    if !real.regions.is_empty() && !regions_written {
                        wrong.push(format!(
                            "{key}: the file holds another language and the record says nothing \
                             about regions"
                        ));
                    }
                }
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    #[test]
    fn a_counter_nobody_photographed_reads_as_no_record_and_no_error() {
        let dir = env::temp_dir().join("linejudge-no_record_here");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let record =
            RecordedAnswers::read(slice::from_ref(&dir), "cloc", &read_the_shipped_dialects())
                .unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert!(record.is_none());
    }

    #[test]
    fn the_answers_are_found_by_case_and_way_of_counting() {
        let record = read_a_record("a_record_that_reads", ONE_FILE).unwrap().unwrap();
        assert_eq!(record.counter, "tokei");
        assert_eq!(record.version, "tokei 14.0.0");
        let pass = record.find("0400-a_case", "default").unwrap();
        assert!(!pass.is_known_failure);
        assert_eq!(pass.counted.as_ref().unwrap().counts.lines, 2);
        let failure = record.find("0500-a_failing_case", "default").unwrap();
        assert!(failure.is_known_failure);
        assert!(failure.note.as_ref().unwrap().contains("swallowed"));
        assert!(record.find("0400-a_case", "strict").is_none());
        let spoken: Vec<(&str, &str)> = record.cases_spoken_about().collect();
        assert_eq!(spoken, [("0400-a_case", "default"), ("0500-a_failing_case", "default")]);
    }

    #[test]
    fn a_note_is_free_in_both_states_and_never_demanded() {
        let noted = ONE_FILE.replace(
            "counted = { lines = 2, code = 1, comments = 1, blanks = 0 }",
            "counted = { lines = 2, code = 1, comments = 1, blanks = 0 }\nnote = \"worth saying\"",
        );
        let record = read_a_record("a_note_on_a_pass", &noted).unwrap().unwrap();
        assert_eq!(record.find("0400-a_case", "default").unwrap().note.as_deref(), Some("worth saying"));

        let silent = ONE_FILE.replace("note = \"\"\"\nthe line comment is swallowed by the block above it\"\"\"\n", "");
        let record = read_a_record("a_failure_with_no_note", &silent).unwrap().unwrap();
        assert_eq!(record.find("0500-a_failing_case", "default").unwrap().note, None);
    }

    #[test]
    fn a_record_under_a_name_that_is_not_its_own_is_refused() {
        let refused = read_a_broken_record(
            "a_record_under_the_wrong_name",
            &ONE_FILE.replace("counter = \"tokei\"", "counter = \"scc\""),
        );
        assert!(refused[0].contains("the file named after it"), "{refused:?}");
    }

    #[test]
    fn a_record_with_no_version_is_refused() {
        let refused = read_a_broken_record(
            "a_record_with_no_version",
            &ONE_FILE.replace("version = \"tokei 14.0.0\"", "version = \" \""),
        );
        assert!(refused[0].contains("names no version"), "{refused:?}");
    }

    #[test]
    fn a_block_of_a_way_of_counting_the_counter_has_not_is_refused() {
        let refused = read_a_broken_record(
            "a_record_of_an_unknown_dialect",
            &ONE_FILE.replace("[answer.0400-a_case.default]", "[answer.0400-a_case.strict]"),
        );
        assert!(refused[0].contains("no way of counting tokei has"), "{refused:?}");
    }

    #[test]
    fn a_bucket_the_dialect_has_not_is_refused_and_so_is_a_language_named_twice() {
        let wrong_bucket = read_a_broken_record(
            "a_record_with_a_wrong_bucket",
            &ONE_FILE.replace("comments = 1, blanks = 0 }", "comments = 1, extra = 0 }"),
        );
        assert!(wrong_bucket[0].contains("has no blanks bucket"), "{wrong_bucket:?}");

        let twice = ONE_FILE.replace(
            "counted = { lines = 2, code = 1, comments = 1, blanks = 0 }",
            "counted = { lines = 2, code = 1, comments = 1, blanks = 0 }\n\
             counted-regions = [\n\
                { language = \"CSS\", lines = 1, code = 1, comments = 0, blanks = 0 },\n\
                { language = \"CSS\", lines = 1, code = 1, comments = 0, blanks = 0 },\n]",
        );
        let refused = read_a_broken_record("a_record_naming_a_language_twice", &twice);
        assert!(refused[0].contains("names CSS twice"), "{refused:?}");
    }

    #[test]
    fn claiming_no_such_file_excludes_an_answer_and_the_failure_flag() {
        let answers = ONE_FILE.replace(
            "counted = { lines = 2, code = 1, comments = 1, blanks = 0 }",
            "unclaimed = true\ncounted = { lines = 2, code = 1, comments = 1, blanks = 0 }",
        );
        let refused = read_a_broken_record("an_unclaimed_record_that_answers", &answers);
        assert!(refused[0].contains("still answers"), "{refused:?}");

        let flagged = ONE_FILE.replace(
            "counted = { lines = 2, code = 1, comments = 1, blanks = 0 }",
            "unclaimed = true\nis-known-failure = true",
        );
        let refused = read_a_broken_record("an_unclaimed_record_flagged", &flagged);
        assert!(refused[0].contains("an answer of its own"), "{refused:?}");

        let plain = ONE_FILE.replace(
            "counted = { lines = 2, code = 1, comments = 1, blanks = 0 }",
            "unclaimed = true",
        );
        let record = read_a_record("an_unclaimed_record", &plain).unwrap().unwrap();
        assert!(record.find("0400-a_case", "default").unwrap().counted.is_none());
    }

    #[test]
    fn an_exception_is_read_beside_the_answers_and_is_refused_without_a_note() {
        let with = ONE_FILE.to_string()
            + "\n[exception.0600-a_deliberate_reading.default]\n\
               expected = { lines = 3, code = 3, comments = 0, blanks = 0 }\n\
               note = \"\"\"\nit reads the whole heredoc as code on purpose\"\"\"\n";
        let record = read_a_record("a_record_with_an_exception", &with).unwrap().unwrap();
        let exception = record.find_exception("0600-a_deliberate_reading", "default").unwrap();
        assert_eq!(exception.expected.counts.buckets["code"], 3);
        assert!(record.find_exception("0400-a_case", "default").is_none());

        let unexplained = with.replace(
            "note = \"\"\"\nit reads the whole heredoc as code on purpose\"\"\"\n",
            "note = \" \"\n",
        );
        let refused = read_a_broken_record("an_exception_with_no_note", &unexplained);
        assert!(refused[0].contains("claims intent"), "{refused:?}");
    }

    #[test]
    fn writing_down_the_right_answer_is_refused_as_an_unknown_field() {
        let with_real = ONE_FILE.replace(
            "counted = { lines = 2, code = 1, comments = 1, blanks = 0 }",
            "real = { lines = 2, code = 1, comments = 1, blanks = 0 }\n\
             counted = { lines = 2, code = 1, comments = 1, blanks = 0 }",
        );
        let refused = read_a_broken_record("a_record_writing_its_own_real", &with_real);
        assert!(refused[0].contains("unknown field `real`"), "{refused:?}");
    }

    #[test]
    fn the_same_build_is_the_same_version_and_never_the_unknown_one() {
        assert!(is_same_build("scc version 3.7.0", "scc version 3.7.0"));
        assert!(!is_same_build("scc version 3.7.0", "scc version 4.0.0"));
        assert!(!is_same_build("unknown version", "unknown version"));
    }

    fn read_a_record(name: &str, text: &str) -> Result<Option<RecordedAnswers>, Vec<String>> {
        let dir = env::temp_dir().join(format!("linejudge-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("tokei.toml"), text).unwrap();
        let read =
            RecordedAnswers::read(slice::from_ref(&dir), "tokei", &read_the_shipped_dialects());
        fs::remove_dir_all(&dir).unwrap();
        read
    }

    fn read_a_broken_record(name: &str, text: &str) -> Vec<String> {
        match read_a_record(name, text) {
            Ok(_) => panic!("the record was read without a fault"),
            Err(faults) => faults,
        }
    }
}
