use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::Write;
use std::path::Path;

use linejudge::adapter::Adapter;
use linejudge::answer::{Answer, Counts};
use linejudge::corpus::Corpus;
use linejudge::dialects::Dialects;
use linejudge::recorded::{Exception, RecordedAnswer, RecordedAnswers};
use linejudge::verdict::{Outcome, measure_and_judge_every_case};

use crate::Trouble;
use crate::style;

const EXTENSION: &str = "toml";

/// Runs the counter over every case and writes its file under `recorded/` from scratch, since a
/// new release of a tool must not mean editing eighty entries by hand.
///
/// A note is a sentence somebody wrote about what this counter does on this case, so it is kept
/// exactly as long as the answer it was written about and dropped the moment that answer moves.
/// Every note dropped is named, because the sentence is still owed by a person.
pub fn record_one_counter(
    out: &mut dyn Write,
    adapter: &Adapter,
    binary: &Path,
    corpus: &Corpus,
    dialects: &Dialects,
    held: Option<&RecordedAnswers>,
    dir: &Path,
) -> Result<(), Trouble> {
    let counter = &adapter.name_of_counter;
    let version = adapter.read_version_or_unknown(binary);
    let mut measured = BTreeMap::new();
    let mut broke = Vec::new();
    let mut dropped = Vec::new();

    for way in &adapter.dialects {
        let rules = dialects
            .find(counter, &way.name)
            .ok_or_else(|| format!("{counter}.{} has no dialect file", way.name))?;
        let judged =
            measure_and_judge_every_case(adapter, way, rules, binary, corpus, None, &version)
                .map_err(|faults| faults.join("\n"))?;
        for one in judged {
            let outcome = match one.outcome {
                Outcome::Broke(message) => {
                    broke.push(format!("{}.{}: {message}", one.case.name, way.name));
                    continue;
                }
                Outcome::Measured(outcome) => outcome,
            };
            let held_entry = held.and_then(|held| held.find(&one.case.name, &way.name));
            let expected = held
                .and_then(|held| held.find_exception(&one.case.name, &way.name))
                .map(|exception| &exception.expected)
                .unwrap_or(&outcome.real);
            let (note, was_dropped) = decide_the_note(held_entry, &outcome.live);
            if was_dropped {
                dropped.push(format!("{}.{}", one.case.name, way.name));
            }
            measured.insert(
                (one.case.name.clone(), way.name.clone()),
                Recorded {
                    is_known_failure: outcome.live.as_ref().is_some_and(|live| live != expected),
                    wants_regions: !expected.regions.is_empty(),
                    counted: outcome.live,
                    note,
                },
            );
        }
    }
    if !broke.is_empty() {
        return Err(Trouble::Said(format!(
            "{counter} broke on {} of the cases, and a photograph with a hole in it is not one:\n  {}",
            broke.len(),
            broke.join("\n  ")
        )));
    }

    let path = dir.join(format!("{counter}.{EXTENSION}"));
    let written = format_the_file(counter, &version, corpus, adapter, dialects, &measured, held)?;
    fs::write(&path, &written)
        .map_err(|e| format!("{} could not be written: {e}", path.display()))?;

    writeln!(out, "\n{}", style::HEADING.paint(&format!("{} at [{version}]", path.display())))?;
    writeln!(out, "  {} answers over {} cases", measured.len(), corpus.cases.len())?;
    for name in name_what_the_corpus_no_longer_holds(corpus, held) {
        writeln!(out, "  {}", style::RECORDED.paint(&format!("dropped, no such case   {name}")))?;
    }
    for name in &dropped {
        writeln!(out, "  {}", style::DIFFERS.paint(&format!("note dropped, it answers differently now   {name}")))?;
    }
    Ok(())
}

/// One block of the file being written, as the run found it.
struct Recorded {
    counted: Option<Answer>,
    is_known_failure: bool,
    /// Whether the case holds another language at all: where it does, the regions are written out
    /// even when the counter found none, so that "it looked and saw nothing" is on the page.
    wants_regions: bool,
    note: Option<String>,
}

/// The note and whether one was thrown away, which is what a person is told so they can write the
/// sentence the new answer needs.
fn decide_the_note(held: Option<&RecordedAnswer>, live: &Option<Answer>) -> (Option<String>, bool) {
    let Some(held) = held else { return (None, false) };
    match held.counted == *live {
        true => (held.note.clone(), false),
        false => (None, held.note.is_some()),
    }
}

fn format_the_file(
    counter: &str,
    version: &str,
    corpus: &Corpus,
    adapter: &Adapter,
    dialects: &Dialects,
    measured: &BTreeMap<(String, String), Recorded>,
    held: Option<&RecordedAnswers>,
) -> Result<String, String> {
    let mut text = format!("counter = {}\nversion = {}\n", quote(counter), quote(version));
    for case in &corpus.cases {
        for way in &adapter.dialects {
            let buckets = match dialects.find(counter, &way.name) {
                Some(rules) => &rules.buckets,
                None => continue,
            };
            let key = (case.name.clone(), way.name.clone());
            if let Some(recorded) = measured.get(&key) {
                text.push('\n');
                write_the_answer(&mut text, &key, recorded, buckets)?;
            }
            if let Some(exception) = held.and_then(|held| held.find_exception(&key.0, &key.1)) {
                text.push('\n');
                write_the_exception(&mut text, &key, exception, buckets)?;
            }
        }
    }
    Ok(text)
}

fn write_the_answer(
    text: &mut String,
    key: &(String, String),
    recorded: &Recorded,
    buckets: &[String],
) -> Result<(), String> {
    let _ = writeln!(text, "[answer.{}.{}]", key.0, key.1);
    let Some(counted) = &recorded.counted else {
        let _ = writeln!(text, "unclaimed = true");
        return write_the_note(text, key, recorded.note.as_deref());
    };
    if recorded.is_known_failure {
        let _ = writeln!(text, "is-known-failure = true");
    }
    let _ = writeln!(text, "counted = {}", format_counts(&counted.counts, buckets));
    write_the_regions(text, "counted-regions", counted, recorded.wants_regions, buckets);
    write_the_note(text, key, recorded.note.as_deref())
}

fn write_the_exception(
    text: &mut String,
    key: &(String, String),
    exception: &Exception,
    buckets: &[String],
) -> Result<(), String> {
    let _ = writeln!(text, "[exception.{}.{}]", key.0, key.1);
    let _ = writeln!(text, "expected = {}", format_counts(&exception.expected.counts, buckets));
    write_the_regions(text, "expected-regions", &exception.expected, false, buckets);
    write_the_note(text, key, Some(&exception.note))
}

fn write_the_regions(
    text: &mut String,
    named: &str,
    answer: &Answer,
    even_when_empty: bool,
    buckets: &[String],
) {
    if answer.regions.is_empty() {
        if even_when_empty {
            let _ = writeln!(text, "{named} = []");
        }
        return;
    }
    let _ = writeln!(text, "{named} = [");
    for region in &answer.regions {
        let counts = Counts { lines: region.lines, buckets: region.buckets.clone() };
        let numbers = name_the_numbers_of(&counts, buckets).join(", ");
        let _ = writeln!(text, "    {{ language = {}, {numbers} }},", quote(&region.language));
    }
    let _ = writeln!(text, "]");
}

/// The note is written back with its own line breaks, so a sentence somebody wrapped by hand stays
/// wrapped. A note carrying the delimiter itself would produce a file this program cannot read,
/// which is refused here rather than written out.
fn write_the_note(
    text: &mut String,
    key: &(String, String),
    note: Option<&str>,
) -> Result<(), String> {
    let Some(note) = note else { return Ok(()) };
    if note.contains("\"\"\"") {
        return Err(format!("the note on {}.{} holds a \"\"\", which cannot be written", key.0, key.1));
    }
    let _ = writeln!(text, "note = \"\"\"\n{note}\"\"\"");
    Ok(())
}

fn format_counts(counts: &Counts, buckets: &[String]) -> String {
    format!("{{ {} }}", name_the_numbers_of(counts, buckets).join(", "))
}

/// In the order the dialect declares its buckets, so that its file and this one read the same way
/// down the page.
fn name_the_numbers_of(counts: &Counts, buckets: &[String]) -> Vec<String> {
    let mut named = vec![format!("lines = {}", counts.lines)];
    named.extend(
        buckets
            .iter()
            .filter_map(|name| counts.buckets.get(name).map(|value| format!("{name} = {value}"))),
    );
    named
}

fn name_what_the_corpus_no_longer_holds(
    corpus: &Corpus,
    held: Option<&RecordedAnswers>,
) -> Vec<String> {
    let Some(held) = held else { return Vec::new() };
    let mut named: Vec<String> = held
        .cases_spoken_about()
        .filter(|(case, _)| !corpus.cases.iter().any(|held| held.name == *case))
        .map(|(case, dialect)| format!("{case}.{dialect}"))
        .collect();
    named.dedup();
    named
}

fn quote(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use linejudge::answer::RegionCounts;

    use super::*;

    fn an_answer(lines: u32, code: u32, languages: &[&str]) -> Answer {
        let buckets = |lines| BTreeMap::from([("code".to_string(), lines)]);
        Answer {
            counts: Counts { lines, buckets: buckets(code) },
            regions: languages
                .iter()
                .map(|language| RegionCounts {
                    language: language.to_string(),
                    lines: 1,
                    buckets: buckets(1),
                })
                .collect(),
        }
    }

    fn a_record(counted: Option<Answer>, note: Option<&str>) -> RecordedAnswer {
        RecordedAnswer {
            counted,
            is_known_failure: false,
            note: note.map(|note| note.to_string()),
        }
    }

    #[test]
    fn a_note_outlives_the_answer_it_was_written_about_and_nothing_more() {
        let counted = an_answer(2, 1, &[]);
        let held = a_record(Some(counted.clone()), Some("it counts the closer twice"));
        let same = decide_the_note(Some(&held), &Some(counted));
        assert_eq!(same, (Some("it counts the closer twice".to_string()), false));

        let moved = decide_the_note(Some(&held), &Some(an_answer(2, 2, &[])));
        assert_eq!(moved, (None, true), "a note about numbers that moved is not kept");

        let unclaimed = decide_the_note(Some(&held), &None);
        assert_eq!(unclaimed, (None, true), "a counter that stopped claiming the file answers anew");

        let quiet = a_record(Some(an_answer(2, 1, &[])), None);
        assert_eq!(decide_the_note(Some(&quiet), &None), (None, false), "nothing was dropped");
        assert_eq!(decide_the_note(None, &None), (None, false));
    }

    #[test]
    fn a_block_of_two_regions_is_written_as_a_document_this_program_can_read_back() {
        let buckets = ["code".to_string()];
        let recorded = Recorded {
            counted: Some(an_answer(4, 2, &["CSS", "JavaScript"])),
            is_known_failure: true,
            wants_regions: true,
            note: Some("the second block is read as the first".to_string()),
        };
        let key = ("1010-a_case".to_string(), "default".to_string());
        let mut text = String::new();
        write_the_answer(&mut text, &key, &recorded, &buckets).unwrap();

        let read: toml::Value = toml::from_str(&text)
            .unwrap_or_else(|e| panic!("what was written does not parse: {e}\n{text}"));
        let block = &read["answer"]["1010-a_case"]["default"];
        assert_eq!(block["counted"]["lines"].as_integer(), Some(4));
        assert_eq!(block["counted-regions"].as_array().map(Vec::len), Some(2));
        assert_eq!(block["is-known-failure"].as_bool(), Some(true));
        assert!(block["note"].as_str().unwrap().starts_with("the second"));
    }

    #[test]
    fn a_case_the_counter_does_not_claim_is_written_as_the_answer_it_is() {
        let recorded = Recorded {
            counted: None,
            is_known_failure: false,
            wants_regions: true,
            note: None,
        };
        let key = ("1010-a_case".to_string(), "default".to_string());
        let mut text = String::new();
        write_the_answer(&mut text, &key, &recorded, &["code".to_string()]).unwrap();
        assert_eq!(text, "[answer.1010-a_case.default]\nunclaimed = true\n");
    }

    #[test]
    fn a_note_holding_the_delimiter_is_refused_instead_of_written() {
        let key = ("1010-a_case".to_string(), "default".to_string());
        let mut text = String::new();
        let refused = write_the_note(&mut text, &key, Some("it says \"\"\" for no reason"))
            .err()
            .unwrap_or_else(|| panic!("it was written anyway"));
        assert!(refused.contains("cannot be written"), "{refused}");
    }
}
