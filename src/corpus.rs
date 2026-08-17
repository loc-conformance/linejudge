use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::answer::{Answer, Counts, RegionCounts};
use crate::deriver::derive_answer;
use crate::dialects::{Dialects, check_buckets};
use crate::readings::{READINGS_FILE, Readings};
use crate::truth::Truth;

const CASE_FILE: &str = "case.toml";
const INPUT_STEM: &str = "input.";
const TRUTH_FILE: &str = "truth.txt";

pub struct Corpus {
    pub cases: Vec<Case>,
}

impl Corpus {
    pub fn read(dir: &Path, dialects: &Dialects) -> Result<Corpus, Vec<Fault>> {
        let readings = match Readings::read(dir) {
            Ok(readings) => readings,
            Err(message) => {
                return Err(vec![Fault { case: READINGS_FILE.to_string(), message }]);
            }
        };
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
            match Case::read(&path, dialects, &readings) {
                Ok(case) => cases.push(case),
                Err(mut found) => faults.append(&mut found),
            }
        }
        // A witness that a broken case fails to be would be blamed twice, so the check waits for
        // a corpus whose cases all read.
        if faults.is_empty() {
            check_witnesses(&readings, &cases, &mut faults);
        }
        if faults.is_empty() {
            Ok(Corpus { cases })
        } else {
            Err(faults)
        }
    }
}

/// `name` is the whole directory name, number and words together, and it is how a case is named in
/// the report and in a known-failures file: the number alone stops naming the same case the day the
/// corpus is renumbered.
pub struct Case {
    pub name: String,
    pub input_file: PathBuf,
    pub trap: String,
    pub answers: Vec<AnswerBlock>,
    pub truth: Truth,
}

impl Case {
    pub fn read(dir: &Path, dialects: &Dialects, readings: &Readings) -> Result<Case, Vec<Fault>> {
        let name = dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| dir.display().to_string());
        let one = |message: String| vec![Fault { case: name.clone(), message }];

        let input_file = match find_input_file_in(dir) {
            Ok(path) => path,
            Err(message) => return Err(one(message)),
        };
        let text = match fs::read_to_string(&input_file) {
            Ok(text) => text,
            Err(error) => return Err(one(format!("the input could not be read: {error}"))),
        };

        let declaration = match fs::read_to_string(dir.join(CASE_FILE)) {
            Ok(text) => text,
            Err(error) => return Err(one(format!("{CASE_FILE} could not be read: {error}"))),
        };
        let raw: RawCase = match toml::from_str(&declaration) {
            Ok(raw) => raw,
            Err(error) => return Err(one(format!("{CASE_FILE} does not parse: {error}"))),
        };

        let mut answers = Vec::new();
        let mut faults = Vec::new();
        let marked = match fs::read_to_string(dir.join(TRUTH_FILE)) {
            Ok(marked) => marked,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Err(one(format!("{TRUTH_FILE} is not there, and a case is its spans")));
            }
            Err(error) => return Err(one(format!("{TRUTH_FILE} could not be read: {error}"))),
        };
        let truth = match Truth::read(&marked, &text) {
            Ok(truth) => truth,
            Err(messages) => {
                return Err(messages
                    .into_iter()
                    .map(|message| Fault {
                        case: name.clone(),
                        message: format!("{TRUTH_FILE}: {message}"),
                    })
                    .collect());
            }
        };
        for reading in truth.find_optional_readings() {
            if readings.find(reading).is_none() {
                faults.push(Fault {
                    case: name.clone(),
                    message: format!(
                        "{TRUTH_FILE} marks the reading {reading}, which {READINGS_FILE} does \
                         not define"
                    ),
                });
            }
        }
        for (counter, blocks) in raw.answer {
            for (dialect, raw) in blocks {
                match AnswerBlock::of(counter.clone(), dialect, raw, &truth, dialects, readings) {
                    Ok(answer) => answers.push(answer),
                    Err(messages) => faults.extend(
                        messages.into_iter().map(|message| Fault { case: name.clone(), message }),
                    ),
                }
            }
        }
        answers.sort_by(|a, b| (&a.name_of_counter, &a.dialect).cmp(&(&b.name_of_counter, &b.dialect)));
        if raw.trap.trim().is_empty() {
            faults.push(Fault { case: name.clone(), message: "the trap says nothing".to_string() });
        }
        if !faults.is_empty() {
            return Err(faults);
        }

        Ok(Case { name, input_file, trap: raw.trap, answers, truth })
    }

    pub fn find_answer_block(&self, counter: &str, dialect: &str) -> Option<&AnswerBlock> {
        self.answers.iter().find(|a| a.name_of_counter == counter && a.dialect == dialect)
    }
}

/// One `[answer.<counter>.<dialect>]` block of a case file: everything the case has to say about
/// one counter's one way of counting this file.
pub struct AnswerBlock {
    pub name_of_counter: String,
    pub dialect: String,
    /// What this counter ought to answer for this file, worked out from the case's marked spans by
    /// that counter's own rules. A counter that claims no such file has one all the same: it says
    /// what the file is, not what the counter did with it.
    pub real: Answer,
    /// What the case records this counter printed. `None` is a counter that does not claim the
    /// file, which is not the same as answering zero and is not a failure of any kind.
    pub counted: Option<Answer>,
    /// Why the two differ, which the case file has to say wherever they do.
    pub note: Option<String>,
}

impl AnswerBlock {
    /// Everything a case file is held to about one counter is decided here, and a block that
    /// breaks more than one rule says so once for each.
    fn of(
        name_of_counter: String,
        dialect: String,
        raw: RawAnswer,
        truth: &Truth,
        dialects: &Dialects,
        readings: &Readings,
    ) -> Result<AnswerBlock, Vec<String>> {
        let key = format!("{name_of_counter}.{dialect}");
        let Some(found) = dialects.find(&name_of_counter, &dialect) else {
            return Err(vec![format!("{key} is a block of no counter this suite knows")]);
        };
        let real = derive_answer(truth, found, readings)?.real;

        if raw.unclaimed {
            if raw.counted.is_some() || raw.counted_regions.is_some() || raw.note.is_some() {
                return Err(vec![format!("{key} claims no such file and still answers")]);
            }
            return Ok(AnswerBlock { name_of_counter, dialect, real, counted: None, note: None });
        }
        let Some(counted) = raw.counted else {
            return Err(vec![format!("{key} writes down no answer and does not say it claims no \
                                     such file")]);
        };
        // A file with another language inside it is a file with something to get wrong there, so
        // leaving the list out would read as "this counter found none" where it means "nobody
        // wrote it down". An empty list is a real answer and stays allowed everywhere.
        let mut faults = Vec::new();
        if !real.regions.is_empty() && raw.counted_regions.is_none() {
            let inside: Vec<&str> =
                real.regions.iter().map(|region| region.language.as_str()).collect();
            faults.push(format!(
                "{key} writes down no counted-regions, and this file holds {}",
                inside.join(", ")
            ));
        }
        let counted =
            Answer { counts: counted.into(), regions: regions_of(raw.counted_regions) };

        if let Err(wrong) = check_buckets(&counted.counts.buckets, &found.buckets) {
            faults.push(format!("{key}.counted {wrong}"));
        }
        for region in &counted.regions {
            if let Err(wrong) = check_buckets(&region.buckets, &found.buckets) {
                faults.push(format!("{key} region {} {wrong}", region.language));
            }
        }
        // No counter reports one language twice for one file, so a repeated language is a copy of
        // a line rather than an answer, and it would fail the case for ever.
        for pair in counted.regions.windows(2) {
            if pair[0].language == pair[1].language {
                faults.push(format!("{key}.counted names {} twice", pair[0].language));
            }
        }

        let says_something = raw.note.as_ref().is_some_and(|note| !note.trim().is_empty());
        match (real == counted, says_something) {
            (true, true) => faults.push(format!("{key} carries a note and answers correctly")),
            (false, false) => faults.push(format!("{key} answers differently and says nothing")),
            _ => {}
        }
        if !faults.is_empty() {
            return Err(faults);
        }
        Ok(AnswerBlock { name_of_counter, dialect, real, counted: Some(counted), note: raw.note })
    }
}

impl From<RawCounts> for Counts {
    fn from(raw: RawCounts) -> Counts {
        Counts { lines: raw.lines, buckets: raw.buckets }
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

/// A case is one directory holding one `input.<extension>`, and that file is the whole of what a
/// counter is ever pointed at.
fn find_input_file_in(dir: &Path) -> Result<PathBuf, String> {
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

fn check_witnesses(readings: &Readings, cases: &[Case], faults: &mut Vec<Fault>) {
    for (name, reading) in readings.iter() {
        let one = |message: String| Fault { case: READINGS_FILE.to_string(), message };
        match cases.iter().find(|case| case.name == reading.witness) {
            None => faults.push(one(format!(
                "{name} names {} as its witness, and there is no case of that name",
                reading.witness
            ))),
            Some(case) if !case.truth.find_optional_readings().contains(&name.as_str()) => {
                faults.push(one(format!(
                    "{name} names {} as its witness, and that case marks no such reading",
                    reading.witness
                )));
            }
            Some(_) => {}
        }
    }
}

fn regions_of(raw: Option<Vec<RawRegionCounts>>) -> Vec<RegionCounts> {
    let mut regions: Vec<RegionCounts> =
        raw.unwrap_or_default().into_iter().map(RegionCounts::from).collect();
    regions.sort();
    regions
}

// These match a case file key for key. The types at the top of the file match a case that has been
// read and checked. Two shapes because the file is allowed to say less: a block for a counter that
// claims no such file has no counts under it, and a file with nothing but its own language in it
// has no region list.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCase {
    trap: String,
    answer: BTreeMap<String, BTreeMap<String, RawAnswer>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAnswer {
    #[serde(default)]
    unclaimed: bool,
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

    use crate::dialects::read_the_shipped_dialects;

    use super::*;

    // Two lines whose right answer the rules work out as one comment and one code line, so a
    // `counted` saying anything else is a case file that has to carry a note.
    const ONE_CASE: &str = r#"
trap = """
a line comment inside a block comment is part of the block"""

[answer.tokei.default]
counted = { lines = 2, code = 1, comments = 1, blanks = 0 }
"#;

    const ONE_CASE_WITH_A_REGION: &str = r#"
trap = """
a script block inside a page"""

[answer.tokei.default]
counted = { lines = 3, code = 3, comments = 0, blanks = 0 }
"#;

    #[test]
    fn every_case_of_the_corpus_is_read_without_a_fault() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("cases");
        match Corpus::read(&dir, &read_the_shipped_dialects()) {
            Ok(corpus) => assert!(!corpus.cases.is_empty(), "{} holds no case", dir.display()),
            Err(faults) => {
                let report: Vec<String> = faults.iter().map(|f| f.to_string()).collect();
                panic!("{}", report.join("\n"));
            }
        }
    }

    #[test]
    fn a_case_that_carries_no_truth_is_refused() {
        let root = env::temp_dir().join("linejudge-a_case_with_no_truth");
        let dir = root.join("0400-a_case_built_by_a_test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("input.c"), "/* a block\n*/ int x = 1;\n").unwrap();
        fs::write(dir.join(CASE_FILE), ONE_CASE).unwrap();
        let faults = Corpus::read(&root, &read_the_shipped_dialects())
            .err()
            .unwrap_or_else(|| panic!("it was read anyway"));
        fs::remove_dir_all(&root).unwrap();
        assert!(faults[0].message.contains("is not there"), "{faults:?}");
    }

    #[test]
    fn every_case_of_the_corpus_answers_every_counter_this_suite_knows() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("cases");
        let dialects = read_the_shipped_dialects();
        let corpus =
            Corpus::read(&dir, &dialects).unwrap_or_else(|_| panic!("the corpus does not read"));
        let mut silent = Vec::new();
        for case in &corpus.cases {
            for dialect in dialects.iter() {
                if case.find_answer_block(&dialect.counter, &dialect.name).is_none() {
                    silent.push(format!(
                        "{} says nothing about {}.{}",
                        case.name, dialect.counter, dialect.name
                    ));
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

    // A counter that finds no region at all in a file that holds one is a real answer, and 5400's
    // tokei is that answer, so an empty list has to stay allowed. What is refused is saying
    // nothing, where the reader cannot tell "found none" from "nobody wrote it down".
    #[test]
    fn a_file_holding_another_language_is_answered_about_that_language_too() {
        let silent = read_the_case_with_a_region("a_region_left_unanswered", ONE_CASE_WITH_A_REGION);
        let faults = silent.err().unwrap_or_else(|| panic!("the case was read without a fault"));
        assert!(faults.iter().any(|f| f.message.contains("this file holds JavaScript")), "{faults:?}");

        let found = ONE_CASE_WITH_A_REGION.to_string()
            + "counted-regions = [{ language = \"JavaScript\", lines = 1, code = 1, comments = 0, blanks = 0 }]\n";
        assert!(read_the_case_with_a_region("a_region_answered", &found).is_ok());

        let none_found = ONE_CASE_WITH_A_REGION.to_string()
            + "counted-regions = []\nnote = \"\"\"\nit reads the page as one language\"\"\"\n";
        assert!(read_the_case_with_a_region("a_region_answered_as_none", &none_found).is_ok());
    }

    #[test]
    fn a_truth_marking_a_reading_the_corpus_does_not_define_is_refused() {
        let case = "trap = \"\"\"\na doc comment read as its own language\"\"\"\n\n\
                    [answer.tokei.default]\n\
                    counted = { lines = 2, code = 1, comments = 1, blanks = 0 }\n";
        let faults = build_and_read_the_case(
            "a_reading_nobody_defined",
            case,
            "input.rs",
            "/** doc */\nlet x = 1\n",
            "/** doc */\nCCCcccccUU Markdown (optional js-jsdoc)\nlet x = 1\n... . . .\n",
        )
        .err()
        .unwrap_or_else(|| panic!("the case was read anyway"));
        let wanted = "marks the reading js-jsdoc, which readings.toml does not define";
        assert!(faults.iter().any(|f| f.message.contains(wanted)), "{faults:?}");
    }

    #[test]
    fn the_witness_of_a_reading_has_to_exist_and_to_mark_it() {
        let root = env::temp_dir().join("linejudge-a_witness_that_is_not_there");
        let dir = root.join("0400-a_case_built_by_a_test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("input.c"), "/* a block\n*/ int x = 1;\n").unwrap();
        let marked = "/* a block\nCCcccccccc\n*/ int x = 1;\nUU ... . . ..\n";
        fs::write(dir.join(TRUTH_FILE), marked).unwrap();
        fs::write(dir.join(CASE_FILE), ONE_CASE).unwrap();
        let reading = |witness: &str| {
            format!("[js-jsdoc]\nsentence = \"the body of a JSDoc comment\"\nwitness = \"{witness}\"\n")
        };
        let read_and_refuse = || {
            Corpus::read(&root, &read_the_shipped_dialects())
                .err()
                .unwrap_or_else(|| panic!("it was read anyway"))
        };

        fs::write(root.join("readings.toml"), reading("0500-not_here")).unwrap();
        let missing = read_and_refuse();
        assert!(missing[0].message.contains("there is no case of that name"), "{missing:?}");

        fs::write(root.join("readings.toml"), reading("0400-a_case_built_by_a_test")).unwrap();
        let unmarked = read_and_refuse();
        fs::remove_dir_all(&root).unwrap();
        assert!(unmarked[0].message.contains("marks no such reading"), "{unmarked:?}");
    }

    #[test]
    fn a_case_file_that_still_writes_down_the_right_answer_is_refused() {
        let case = ONE_CASE.replace(
            "[answer.tokei.default]",
            "[answer.tokei.default]\nreal    = { lines = 2, code = 1, comments = 1, blanks = 0 }",
        );
        let faults = read_a_broken_case("a_case_file_writing_its_own_real", &case);
        assert!(faults[0].message.contains("unknown field `real`"), "{faults:?}");
    }

    fn read_the_case(name: &str, declaration: &str) -> Result<Corpus, Vec<Fault>> {
        build_and_read_the_case(
            name,
            declaration,
            "input.c",
            "/* a block\n*/ int x = 1;\n",
            "/* a block\nCCcccccccc\n*/ int x = 1;\nUU ... . . ..\n",
        )
    }

    fn read_the_case_with_a_region(name: &str, declaration: &str) -> Result<Corpus, Vec<Fault>> {
        build_and_read_the_case(
            name,
            declaration,
            "input.html",
            "<script>\nlet x = 1\n</script>\n",
            "<script>\n>>>>>>>> JavaScript\nlet x = 1\n... . . .\n</script>\n<<<<<<<<<\n",
        )
    }

    fn build_and_read_the_case(
        name: &str,
        declaration: &str,
        input_file: &str,
        input: &str,
        marked: &str,
    ) -> Result<Corpus, Vec<Fault>> {
        let root = env::temp_dir().join(format!("linejudge-{name}"));
        let dir = root.join("0400-a_case_built_by_a_test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(input_file), input).unwrap();
        fs::write(dir.join(TRUTH_FILE), marked).unwrap();
        fs::write(dir.join(CASE_FILE), declaration).unwrap();
        let read = Corpus::read(&root, &read_the_shipped_dialects());
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
