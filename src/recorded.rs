//! What a counter printed the last time it was measured, kept so that a change can be noticed.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::Deserialize;

use crate::adapter::{Adapter, Variation};
use crate::answer::{Answer, Counts, RegionCounts};
use crate::dialects::check_buckets;
use crate::faults::Faults;

/// The directory the records are read from, one `<counter>.toml` inside it per counter.
pub const RECORDED_DIR: &str = "recorded";
const RECORDED_EXTENSION: &str = "toml";

/// What one counter printed for each case on the day it was measured, at the version written at
/// the top of the file. A counter with no such record is measured all the same: the record adds
/// only the question of whether the counter still answers the way it did.
#[derive(Debug)]
pub struct RecordedAnswers {
    /// The counter these answers came from.
    pub counter: String,
    /// The version line the measured binary printed, kept whole.
    pub version: String,
    /// The linejudge that wrote the record, absent on records older than the stamp.
    pub measured_with: Option<String>,
    answers: BTreeMap<(String, String), RecordedAnswer>,
    exceptions: BTreeMap<(String, String), Exception>,
    no_longer_declared: Vec<(String, String)>,
    pointing_at_nothing: Vec<(String, String, String)>,
}

impl RecordedAnswers {
    /// Layered like the adapters. The last directory holding a file for this counter is the one
    /// read, and no record at all is the ordinary state of anybody's tool.
    pub fn read(
        dirs: &[PathBuf],
        adapter: &Adapter,
    ) -> Result<Option<RecordedAnswers>, Faults> {
        let name_of_counter = adapter.name_of_counter.as_str();
        let variations = adapter.variations.as_slice();
        let named = format!("{name_of_counter}.{RECORDED_EXTENSION}");
        let Some(path) = dirs.iter().rev().map(|dir| dir.join(&named)).find(|path| path.is_file())
        else {
            return Ok(None);
        };
        let text = fs::read_to_string(&path)
            .map_err(|e| Faults::from(format!("{} could not be read: {e}", path.display())))?;
        let raw: RawRecorded = toml::from_str(&text)
            .map_err(|e| format!("{} does not parse: {e}", path.display()))?;
        let where_it_is = path.display();
        let mut faults = Vec::new();
        if raw.counter != name_of_counter {
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
        let mut no_longer_declared = Vec::new();
        let mut pointing_at_nothing = Vec::new();
        for (case, blocks) in raw.answer {
            // The blocks that answer for themselves first, so a pointer always has its target.
            let mut pointers = Vec::new();
            for (variation, mut block) in blocks {
                let key = format!("answer.{case}.{variation}");
                let Some(declared) = variations.iter().find(|one| one.name == variation) else {
                    no_longer_declared.push((case.clone(), variation));
                    continue;
                };
                if let Some(target) = block.same_as.take() {
                    pointers.push((variation, block, declared, target));
                    continue;
                }
                match RecordedAnswer::of(block, &declared.buckets) {
                    Ok(answer) => {
                        answers.insert((case.clone(), variation), answer);
                    }
                    Err(found) => faults
                        .extend(found.into_iter().map(|m| format!("{where_it_is}: {key} {m}"))),
                }
            }
            for (variation, block, declared, target) in pointers {
                let key = format!("answer.{case}.{variation}");
                let named = target.clone();
                match resolve_the_pointer(&answers, &case, declared, block, target, variations) {
                    Ok(Some(answer)) => {
                        answers.insert((case.clone(), variation), answer);
                    }
                    Ok(None) => pointing_at_nothing.push((case.clone(), variation, named)),
                    Err(message) => faults.push(format!("{where_it_is}: {key} {message}")),
                }
            }
        }
        let mut exceptions = BTreeMap::new();
        for (case, blocks) in raw.exception {
            for (dialect, block) in blocks {
                let key = format!("exception.{case}.{dialect}");
                // An answer is machine written and `record` makes it again, so a stale one is
                // reported. Nothing can make an exception again, so a stale one stops the read.
                let Some(buckets) = find_the_buckets_of_a_dialect(variations, &dialect) else {
                    faults.push(match variations.iter().find(|one| one.name == dialect) {
                        Some(one) => format!(
                            "{where_it_is}: {key} names the variation {dialect}, and an exception \
                             is declared under the dialect it names, which is {}",
                            one.name_of_dialect
                        ),
                        None => format!(
                            "{where_it_is}: {key} is declared under a dialect {name_of_counter} no \
                             longer has, and an exception is written by hand and lost the moment \
                             it is not read"
                        ),
                    });
                    continue;
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
            return Err(faults.into());
        }
        no_longer_declared.sort();
        pointing_at_nothing.sort();
        Ok(Some(RecordedAnswers {
            counter: raw.counter,
            version: raw.version,
            measured_with: raw.measured_with,
            answers,
            exceptions,
            no_longer_declared,
            pointing_at_nothing,
        }))
    }

    /// What the counter printed for this case in this variation, and `None` where the record
    /// says nothing about it.
    pub fn find(&self, name_of_case: &str, name_of_variation: &str) -> Option<&RecordedAnswer> {
        self.answers.get(&(name_of_case.to_string(), name_of_variation.to_string()))
    }

    /// The exception declared for this case under this dialect, which every variation judged by
    /// those rules is held to. `None` where there is none, which is nearly always.
    pub fn find_exception(&self, name_of_case: &str, name_of_dialect: &str) -> Option<&Exception> {
        self.exceptions.get(&(name_of_case.to_string(), name_of_dialect.to_string()))
    }

    /// Every answer block, as the case it speaks about and the variation it was measured in.
    pub fn name_every_answer_block(&self) -> impl Iterator<Item = (&str, &str)> {
        self.answers.keys().map(|(case, variation)| (case.as_str(), variation.as_str()))
    }

    /// Every exception block, as the case it speaks about and the dialect it is declared under.
    pub fn name_every_exception_block(&self) -> impl Iterator<Item = (&str, &str)> {
        self.exceptions.keys().map(|(case, dialect)| (case.as_str(), dialect.as_str()))
    }

    /// Every answer block keyed by a variation the counter no longer declares. These are held out
    /// and never refused, so that `record` can still rewrite the file.
    pub fn name_every_block_no_longer_declared(&self) -> impl Iterator<Item = (&str, &str)> {
        self.no_longer_declared.iter().map(|(case, named)| (case.as_str(), named.as_str()))
    }

    /// Every block whose `same-as` named something the counter dropped, as case, variation and
    /// the name it pointed at.
    pub fn name_every_block_pointing_at_nothing(
        &self,
    ) -> impl Iterator<Item = (&str, &str, &str)> {
        self.pointing_at_nothing
            .iter()
            .map(|(case, named, target)| (case.as_str(), named.as_str(), target.as_str()))
    }
}

/// What the counter printed for one case in one variation, one entry of that record.
#[derive(Clone, Debug)]
pub struct RecordedAnswer {
    /// What it printed, and `None` where it said there is no such file.
    pub counted: Option<Answer>,
    /// Whether what it printed differs from what its own rules ask for. Written out rather than
    /// worked out, so that a flag the numbers contradict is refused instead of read.
    pub is_known_failure: bool,
    /// A sentence somebody wrote about this answer, kept exactly as long as the answer it was
    /// written about and dropped the moment that answer moves.
    pub note: Option<String>,
    /// Whether that sentence was written about the block this one points at, in which case
    /// nothing may keep it as this answer's own.
    pub note_is_inherited: bool,
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
            return Ok(RecordedAnswer {
                counted: None,
                is_known_failure: false,
                note,
                note_is_inherited: false,
            });
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
            note_is_inherited: false,
        })
    }
}

/// A case whose deliberate behavior under one dialect no rule over marked spans can express.
/// It stands in for the answer the rules would derive, so the case passes and is counted apart.
#[derive(Debug)]
pub struct Exception {
    /// What the counter is held to here instead.
    pub expected: Answer,
    /// Why, and it cannot be left out: an exception claims something about the tool's own intent,
    /// and such a claim with no reason behind it is worth nothing.
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

/// Whether the record and the running binary are the same build.
///
/// A counter that names no version answers the same "unknown version" on both sides and is taken
/// at its word, so its record holds its runs the way a versioned counter's does. Held apart from
/// it and asked for nothing, such a counter could never have a record that judges anything.
pub fn is_same_build(recorded: &str, running: &str) -> bool {
    recorded == running
}

// A case and a variation both become a key of `[answer.<case>.<variation>]`, so a name holding
// anything TOML reads as punctuation makes a file this program writes and then cannot read.
pub(crate) fn check_it_is_a_bare_key(name: &str) -> Result<(), String> {
    let bare = |one: char| one.is_ascii_alphanumeric() || one == '_' || one == '-';
    match name.is_empty() || !name.chars().all(bare) {
        true => Err(format!(
            "{name} is written into a key of the recorded file, so it holds letters, digits, _ \
             and - and nothing else"
        )),
        false => Ok(()),
    }
}

fn resolve_the_pointer(
    answers: &BTreeMap<(String, String), RecordedAnswer>,
    name_of_case: &str,
    pointer: &Variation,
    raw: RawAnswer,
    target: String,
    variations: &[Variation],
) -> Result<Option<RecordedAnswer>, String> {
    if raw.counted.is_some() || raw.counted_regions.is_some() || raw.unclaimed {
        return Err("says same-as and writes an answer of its own beside it".to_string());
    }
    if raw.is_known_failure {
        return Err("says same-as and flags a failure, and the flag comes with the answer"
            .to_string());
    }
    // This line is machine written, so every way the adapter can have moved under it is held out
    // and measured again. A pointer at a variation that leads nothing is a shape it never writes.
    let pointed_at = variations.iter().find(|one| one.name == target);
    if pointer.is_major || pointed_at.is_none_or(|one| one.name_of_dialect != pointer.name_of_dialect)
    {
        return Ok(None);
    }
    if !pointed_at.is_some_and(|one| one.is_major) {
        return Err(format!(
            "points at {target}, which is no major, and a pointer names the one its dialect is \
             scored by"
        ));
    }
    let Some(held) = answers.get(&(name_of_case.to_string(), target.clone())) else {
        return Err(format!("points at {target}, which has no answer of its own for this case"));
    };
    let mut resolved = held.clone();
    resolved.note_is_inherited = resolved.note.is_some();
    if let Some(note) = raw.note.filter(|note| !note.trim().is_empty()) {
        resolved.note = Some(note);
        resolved.note_is_inherited = false;
    }
    Ok(Some(resolved))
}

// Any variation naming it will do, since the buckets belong to the dialect itself.
fn find_the_buckets_of_a_dialect<'a>(
    variations: &'a [Variation],
    name_of_dialect: &str,
) -> Option<&'a [String]> {
    variations
        .iter()
        .find(|one| one.name_of_dialect == name_of_dialect)
        .map(|one| one.buckets.as_slice())
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
    #[serde(rename = "measured-with", default)]
    measured_with: Option<String>,
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
    #[serde(rename = "same-as")]
    same_as: Option<String>,
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

    use crate::adapter::UNKNOWN_VERSION;
    use crate::adapter::{Adapter, is_the_declared_version};
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

    const TWO_VARIATIONS: &str = r#"
counter = "cloc"
version = "2.10"

[answer.0400-a_case.default]
is-known-failure = true
counted = { lines = 2, code = 2, comments = 0, blanks = 0 }
note = """
the opener sits inside a string and cloc reads it as a comment anyway"""

[answer.0400-a_case.stripstr]
same-as = "default"
"#;

    #[test]
    fn a_pointer_is_read_as_the_answer_it_names_and_carries_its_note_and_its_flag() {
        let record = read_a_record_for("cloc", "a_record_with_a_pointer", TWO_VARIATIONS)
            .unwrap_or_else(|faults| panic!("{}", faults.join("\n")))
            .unwrap();
        let major = record.find("0400-a_case", "default").unwrap();
        let minor = record.find("0400-a_case", "stripstr").unwrap();
        assert_eq!(minor.counted, major.counted);
        assert!(minor.is_known_failure, "the flag comes with the answer");
        assert_eq!(minor.note, major.note, "a failure it shares is explained by one sentence");
    }

    #[test]
    fn a_pointer_keeps_a_note_of_its_own_where_it_writes_one() {
        let text = TWO_VARIATIONS.replace(
            "same-as = \"default\"",
            "same-as = \"default\"\nnote = \"it lands there by another road\"",
        );
        let record = read_a_record_for("cloc", "a_pointer_with_its_own_note", &text)
            .unwrap_or_else(|faults| panic!("{}", faults.join("\n")))
            .unwrap();
        let minor = record.find("0400-a_case", "stripstr").unwrap();
        assert_eq!(minor.note.as_deref(), Some("it lands there by another road"));
    }

    #[test]
    fn every_pointer_that_could_mean_two_things_is_refused() {
        let refused = |name: &str, from: &str, to: &str| {
            let text = TWO_VARIATIONS.replace(from, to);
            match read_a_record_for("cloc", name, &text) {
                Err(faults) => faults.join("\n"),
                Ok(_) => panic!("{name} was read"),
            }
        };
        let both = refused(
            "a_pointer_answering_twice",
            "same-as = \"default\"",
            "same-as = \"default\"\ncounted = { lines = 2, code = 2, comments = 0, blanks = 0 }",
        );
        assert!(both.contains("writes an answer of its own beside it"), "{both}");

        let flagged = refused(
            "a_pointer_flagging_a_failure",
            "same-as = \"default\"",
            "same-as = \"default\"\nis-known-failure = true",
        );
        assert!(flagged.contains("the flag comes with the answer"), "{flagged}");

        let at_itself =
            refused("a_pointer_at_a_minor", "same-as = \"default\"", "same-as = \"stripstr\"");
        assert!(at_itself.contains("which is no major"), "{at_itself}");

        let nowhere = refused("a_pointer_at_nothing", "[answer.0400-a_case.default]", "[answer.0400-a_case.nothing]");
        assert!(nowhere.contains("has no answer of its own for this case"), "{nowhere}");
    }

    // Faulting on any of these would stop `record`, the only thing that can rewrite the line.
    #[test]
    fn a_pointer_the_adapter_moved_under_is_held_out_and_named() {
        let held_out = |name: &str, text: &str| {
            let record = read_a_record_for("cloc", name, text)
                .unwrap_or_else(|faults| panic!("{name}: {}", faults.join("\n")))
                .unwrap();
            record
                .name_every_block_pointing_at_nothing()
                .map(|(case, named, target)| format!("{case}.{named} at {target}"))
                .collect::<Vec<_>>()
        };
        let gone = held_out(
            "a_pointer_at_a_gone_name",
            &TWO_VARIATIONS.replace("same-as = \"default\"", "same-as = \"vanished\""),
        );
        assert_eq!(gone, ["0400-a_case.stripstr at vanished"]);

        // The one that was promoted, so the line this program wrote now sits on a major.
        let promoted = held_out(
            "a_major_pointing_away",
            r#"
counter = "cloc"
version = "2.10"

[answer.0400-a_case.stripstr]
is-known-failure = true
counted = { lines = 2, code = 2, comments = 0, blanks = 0 }

[answer.0400-a_case.default]
same-as = "stripstr"
"#,
        );
        assert_eq!(promoted, ["0400-a_case.default at stripstr"]);
    }

    // Completeness is demanded of this suite's own counters and never at run time, where a
    // missing record is the ordinary state of anybody else's tool.
    #[test]
    fn every_roster_counter_records_every_case_and_nothing_contradicts_the_rules() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let dialects = read_the_shipped_dialects();
        let corpus =
            Corpus::read(&root.join("cases")).unwrap_or_else(|faults| panic!("{faults:?}"));
        let adapters = Adapter::read_all(&[root.join("adapters")], &dialects).unwrap();
        let mut wrong = Vec::new();
        for adapter in &adapters {
            let counter = adapter.name_of_counter.as_str();
            let record =
                RecordedAnswers::read(&[root.join(RECORDED_DIR)], adapter)
                    .unwrap_or_else(|faults| panic!("{}", faults.join("\n")))
                    .unwrap_or_else(|| panic!("{counter} has no recorded answers"));
            let text = fs::read_to_string(root.join(RECORDED_DIR).join(format!("{counter}.toml")))
                .unwrap();
            let raw: RawRecorded = toml::from_str(&text).unwrap();
            for variation in &adapter.variations {
                let dialect = dialects.find(counter, &variation.name_of_dialect).unwrap();
                for case in &corpus.cases {
                    let key = format!("{counter}.{}: {}", variation.name, case.name);
                    let Some(entry) = record.find(&case.name, &variation.name) else {
                        wrong.push(format!("{key} has no recorded answer"));
                        continue;
                    };
                    let real = match record.find_exception(&case.name, &variation.name_of_dialect) {
                        Some(exception) => exception.expected.clone(),
                        None => derive_answer(&case.truth, dialect, &corpus.readings)
                            .unwrap_or_else(|faults| panic!("{key}: {faults:?}"))
                            .real,
                    };
                    let Some(counted) = &entry.counted else { continue };
                    if entry.is_known_failure == (*counted == real) {
                        wrong.push(format!("{key}: the flag contradicts the numbers beside it"));
                    }
                    let written = &raw.answer[&case.name][&variation.name];
                    // A pointer writes no regions of its own, and the block it names carries them.
                    let regions_written =
                        written.counted_regions.is_some() || written.same_as.is_some();
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

    // Nothing reaches a dialect file except through a variation, so an unnamed one is invisible.
    #[test]
    fn every_dialect_file_is_named_by_a_variation() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let dialects = read_the_shipped_dialects();
        let adapters = Adapter::read_all(&[root.join("adapters")], &dialects).unwrap();
        let mut orphaned = Vec::new();
        for dialect in dialects.iter() {
            let named = adapters.iter().filter(|one| one.name_of_counter == dialect.counter).any(
                |one| one.variations.iter().any(|two| two.name_of_dialect == dialect.name),
            );
            if !named {
                orphaned.push(format!("{}.{}", dialect.counter, dialect.name));
            }
        }
        assert!(orphaned.is_empty(), "no variation names {}", orphaned.join(", "));
    }

    // The other direction of the test above. A case that was deleted or renamed leaves its answers
    // behind in the recorded files, and no other part of the suite reads them again to notice. A
    // disabled case is still a case and keeps its answers.
    #[test]
    fn a_recorded_answer_names_a_case_that_is_still_there() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let corpus =
            Corpus::read(&root.join("cases")).unwrap_or_else(|faults| panic!("{faults:?}"));
        let mut wrong = Vec::new();
        for counter in collect_the_recorded_counters(root) {
            let text = fs::read_to_string(root.join(RECORDED_DIR).join(format!("{counter}.toml")))
                .unwrap();
            let raw: RawRecorded = toml::from_str(&text).unwrap();
            for name in raw.answer.keys().chain(raw.exception.keys()) {
                let still_there = corpus.cases.iter().any(|case| case.name == *name)
                    || corpus.disabled.contains(name);
                if !still_there {
                    wrong.push(format!("{counter}: {name} is recorded and is no longer a case"));
                }
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    // Raising the version a fetch downloads without re-measuring would publish numbers taken from
    // a build nobody ever recorded.
    #[test]
    fn a_counter_is_downloaded_at_the_version_its_answers_were_recorded_from() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let dialects = read_the_shipped_dialects();
        let adapters = Adapter::read_all(&[root.join("adapters")], &dialects).unwrap();
        let mut wrong = Vec::new();
        for adapter in &adapters {
            let counter = &adapter.name_of_counter;
            let Some(how) = &adapter.acquisition else { continue };
            let record =
                RecordedAnswers::read(&[root.join(RECORDED_DIR)], adapter)
                    .unwrap_or_else(|faults| panic!("{}", faults.join("\n")))
                    .unwrap_or_else(|| panic!("{counter} has no recorded answers"));
            if !is_the_declared_version(&how.version, &record.version) {
                wrong.push(format!(
                    "{counter} is downloaded at {} and its answers came from \"{}\"",
                    how.version, record.version
                ));
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
            RecordedAnswers::read(slice::from_ref(&dir), &the_shipped_adapter_of("cloc")).unwrap();
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
        let spoken: Vec<(&str, &str)> = record.name_every_answer_block().collect();
        assert_eq!(spoken, [("0400-a_case", "default"), ("0500-a_failing_case", "default")]);
        assert_eq!(record.name_every_exception_block().count(), 0);
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

    // Renaming a variation is a change the adapter is allowed to make, and faulting here would
    // leave `record`, the command that rewrites the file, refusing to run over a stale one.
    #[test]
    fn a_block_naming_no_variation_is_held_out_and_named_never_refused() {
        let record = read_a_record(
            "a_record_of_a_dropped_variation",
            &ONE_FILE.replace("[answer.0400-a_case.default]", "[answer.0400-a_case.strict]"),
        )
        .unwrap_or_else(|faults| panic!("{faults:?}"))
        .unwrap();
        assert!(record.find("0400-a_case", "strict").is_none());
        assert!(record.find("0500-a_failing_case", "default").is_some());
        let dropped: Vec<(&str, &str)> = record.name_every_block_no_longer_declared().collect();
        assert_eq!(dropped, [("0400-a_case", "strict")]);
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
    fn the_same_build_is_the_same_version_and_a_counter_naming_none_is_its_own() {
        assert!(is_same_build("scc version 3.7.0", "scc version 3.7.0"));
        assert!(!is_same_build("scc version 3.7.0", "scc version 4.0.0"));
        assert!(is_same_build(UNKNOWN_VERSION, UNKNOWN_VERSION));
        assert!(!is_same_build("scc version 4.0.0", UNKNOWN_VERSION));
        assert!(!is_same_build(UNKNOWN_VERSION, "scc version 4.0.0"));
    }

    // The roster comes off the recorded directory itself, so a counter joining the suite is
    // covered by these tests without anybody remembering to name it here.
    fn collect_the_recorded_counters(root: &Path) -> Vec<String> {
        let mut counters: Vec<String> = fs::read_dir(root.join(RECORDED_DIR))
            .unwrap()
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("toml"))
            .filter_map(|path| path.file_stem().map(|s| s.to_string_lossy().into_owned()))
            .collect();
        counters.sort();
        assert!(!counters.is_empty(), "an empty roster would test nothing and say it passed");
        counters
    }

    fn read_a_record(name: &str, text: &str) -> Result<Option<RecordedAnswers>, Faults> {
        read_a_record_for("tokei", name, text)
    }

    fn read_a_record_for(
        name_of_counter: &str,
        name: &str,
        text: &str,
    ) -> Result<Option<RecordedAnswers>, Faults> {
        let dir = env::temp_dir().join(format!("linejudge-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{name_of_counter}.toml")), text).unwrap();
        let read =
            RecordedAnswers::read(slice::from_ref(&dir), &the_shipped_adapter_of(name_of_counter));
        fs::remove_dir_all(&dir).unwrap();
        read
    }

    fn the_shipped_adapter_of(name_of_counter: &str) -> Adapter {
        let dirs = [Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters")];
        Adapter::read_one(&dirs, name_of_counter, &read_the_shipped_dialects()).unwrap()
    }

    fn read_a_broken_record(name: &str, text: &str) -> Faults {
        match read_a_record(name, text) {
            Ok(_) => panic!("the record was read without a fault"),
            Err(faults) => faults,
        }
    }
}
