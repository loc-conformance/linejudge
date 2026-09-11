use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write};
use std::path::Path;

use linejudge::adapter::UNKNOWN_VERSION;
use linejudge::adapter::{Adapter, Variation};
use linejudge::answer::{Answer, Counts};
use linejudge::corpus::Corpus;
use linejudge::dialects::Dialects;
use linejudge::recorded::{Exception, RecordedAnswer, RecordedAnswers};
use linejudge::verdict::{Outcome, measure_and_judge_every_case};

use crate::Trouble;
use crate::style;

const EXTENSION: &str = "toml";

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
    let mut started = Vec::new();
    let mut stopped = Vec::new();

    for variation in &adapter.variations {
        let judged = measure_and_judge_every_case(
            adapter, variation, dialects, binary, corpus, None, &version,
        )
        .map_err(|faults| faults.join("\n"))?;
        for one in judged {
            let outcome = match one.outcome {
                Outcome::Broke(message) => {
                    broke.push(format!("{}.{}: {message}", one.case.name, variation.name));
                    continue;
                }
                Outcome::Measured(outcome) => outcome,
            };
            let held_entry = held.and_then(|held| held.find(&one.case.name, &variation.name));
            let expected = held
                .and_then(|held| held.find_exception(&one.case.name, &variation.name_of_dialect))
                .map(|exception| &exception.expected)
                .unwrap_or(&outcome.real);
            let (note, was_dropped) = decide_the_note(held_entry, &outcome.live);
            let key = || format!("{}.{}", one.case.name, variation.name);
            if was_dropped {
                dropped.push(key());
            }
            let fails = outcome.live.as_ref().is_some_and(|live| live != expected);
            // Only where an answer was held, since a case with none has no state to have moved.
            if let Some(held_entry) = held_entry {
                match (held_entry.is_known_failure, fails) {
                    (false, true) => started.push(key()),
                    (true, false) => stopped.push(key()),
                    _ => {}
                }
            }
            let inherited = held_entry.is_some_and(|held| held.note_is_inherited);
            measured.insert(
                (one.case.name.clone(), variation.name.clone()),
                Recorded {
                    is_known_failure: fails,
                    wants_regions: !expected.regions.is_empty(),
                    counted: outcome.live,
                    note,
                },
            );
            // Majors are measured first, so the major's answer is in hand. A minor that parts
            // from it loses the sentence it read through it and has none of its own.
            if inherited {
                let mine = &measured[&(one.case.name.clone(), variation.name.clone())];
                let alike =
                    find_the_major_answering_alike(adapter, &measured, &one.case.name, variation, mine);
                if alike.is_none() {
                    dropped.push(key());
                }
            }
        }
    }
    dropped.sort();
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
    // A counter that declares no flag is taken at its word and its record still judges its runs.
    // One that declares a flag and answers nothing has left the stamp off by accident.
    if version == UNKNOWN_VERSION && adapter.version_flag.is_some() {
        writeln!(out, "  {}", style::DIFFERS.paint(
                "it declares a version flag and answered nothing, so this record names no build"))?;
    }
    writeln!(out, "  {} answers over {} cases", measured.len(), corpus.cases.len())?;
    for (name, why) in name_what_the_corpus_no_longer_holds(corpus, held) {
        writeln!(out, "  {}", style::RECORDED.paint(&format!("{why}   {name}")))?;
    }
    // This run is what erases them, so this is the last moment anybody sees that they were there.
    for (case, named) in held.iter().flat_map(|held| held.name_every_block_no_longer_declared()) {
        writeln!(out, "  {}", style::DIFFERS.paint(
                &format!("dropped, the adapter no longer declares it   {case}.{named}")))?;
    }
    for (case, named, target) in
        held.iter().flat_map(|held| held.name_every_block_pointing_at_nothing())
    {
        writeln!(out, "  {}", style::DIFFERS.paint(
                &format!("measured anew, it read its answer through {target}   {case}.{named}")))?;
    }
    write_what_moved(out, &dropped, &started, &stopped)?;
    Ok(())
}

// Read by the version-check workflow, which can tell none of this from the file it just wrote.
fn write_what_moved(
    out: &mut dyn Write,
    dropped: &[String],
    started: &[String],
    stopped: &[String],
) -> io::Result<()> {
    for name in dropped {
        writeln!(out, "  {}", style::DIFFERS.paint(&format!("note dropped, it answers differently now   {name}")))?;
    }
    for name in started {
        writeln!(out, "  {}", style::DIFFERS.paint(&format!("started failing   {name}")))?;
    }
    for name in stopped {
        writeln!(out, "  {}", style::RECORDED.paint(&format!("stopped failing   {name}")))?;
    }
    Ok(())
}

struct Recorded {
    counted: Option<Answer>,
    is_known_failure: bool,
    // Where the case holds another language, the regions are written out even when the counter
    // found none, so that "it looked and saw nothing" is on the page.
    wants_regions: bool,
    note: Option<String>,
}

// The note, and whether one was thrown away, which is what a person is told so they can write the
// sentence the new answer needs.
fn decide_the_note(held: Option<&RecordedAnswer>, live: &Option<Answer>) -> (Option<String>, bool) {
    let Some(held) = held else { return (None, false) };
    // Kept, a note read through a pointer would move onto a run nobody measured it on.
    if held.note_is_inherited {
        return (None, false);
    }
    match held.counted == *live {
        true => (held.note.clone(), false),
        false => (None, held.note.is_some()),
    }
}

fn format_the_file(
    name_of_counter: &str,
    version: &str,
    corpus: &Corpus,
    adapter: &Adapter,
    dialects: &Dialects,
    measured: &BTreeMap<(String, String), Recorded>,
    held: Option<&RecordedAnswers>,
) -> Result<String, String> {
    let mut text = format!(
        "counter = {}\nversion = {}\nmeasured-with = {}\n",
        quote(name_of_counter),
        quote(version),
        quote(&format!("linejudge {}", crate::VERSION))
    );
    let mut per_dialect: Vec<(&str, &[String])> = Vec::new();
    let mut per_variation: Vec<(&Variation, &[String])> = Vec::new();
    for variation in &adapter.variations {
        let Some(rules) = dialects.find(name_of_counter, &variation.name_of_dialect) else {
            return Err(format!(
                "{name_of_counter}.{} was measured and its dialect is gone",
                variation.name
            ));
        };
        per_variation.push((variation, &rules.buckets));
        if !per_dialect.iter().any(|(named, _)| *named == variation.name_of_dialect) {
            per_dialect.push((variation.name_of_dialect.as_str(), &rules.buckets));
        }
    }
    for case in &corpus.cases {
        for (variation, buckets) in &per_variation {
            let key = (case.name.clone(), variation.name.clone());
            if let Some(recorded) = measured.get(&key) {
                text.push('\n');
                let same = find_the_major_answering_alike(
                    adapter, measured, &case.name, variation, recorded,
                );
                write_the_answer(&mut text, &key, recorded, buckets, same)?;
            }
        }
        // Once per dialect, since two variations sharing one would write the same header twice.
        for (name_of_dialect, buckets) in &per_dialect {
            let key = (case.name.clone(), (*name_of_dialect).to_string());
            if let Some(exception) = held.and_then(|held| held.find_exception(&key.0, &key.1)) {
                text.push('\n');
                write_the_exception(&mut text, &key, exception, buckets)?;
            }
        }
    }
    Ok(text)
}

// Its name, and the note standing beside its answer, which is never written down twice.
fn find_the_major_answering_alike<'a>(
    adapter: &'a Adapter,
    measured: &'a BTreeMap<(String, String), Recorded>,
    name_of_case: &str,
    variation: &Variation,
    mine: &Recorded,
) -> Option<(&'a str, Option<&'a str>)> {
    if variation.is_major {
        return None;
    }
    let major = adapter
        .variations
        .iter()
        .find(|one| one.is_major && one.name_of_dialect == variation.name_of_dialect)?;
    let theirs = measured.get(&(name_of_case.to_string(), major.name.clone()))?;
    let alike = theirs.counted == mine.counted && theirs.is_known_failure == mine.is_known_failure;
    alike.then_some((major.name.as_str(), theirs.note.as_deref()))
}

fn write_the_answer(
    text: &mut String,
    key: &(String, String),
    recorded: &Recorded,
    buckets: &[String],
    same_as: Option<(&str, Option<&str>)>,
) -> Result<(), String> {
    let _ = writeln!(text, "[answer.{}.{}]", key.0, key.1);
    if let Some((major, their_note)) = same_as {
        let _ = writeln!(text, "same-as = {}", quote(major));
        let own = recorded.note.as_deref().filter(|note| Some(*note) != their_note);
        return write_the_note(text, key, own);
    }
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

// The note keeps its own line breaks, so a sentence wrapped by hand stays wrapped. A note carrying
// the delimiter would produce a file this program cannot read, so it is refused rather than
// written out.
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

// In the order the dialect declares its buckets, so its file and this one read the same way down
// the page.
fn name_the_numbers_of(counts: &Counts, buckets: &[String]) -> Vec<String> {
    let mut named = vec![format!("lines = {}", counts.lines)];
    named.extend(
        buckets
            .iter()
            .filter_map(|name| counts.buckets.get(name).map(|value| format!("{name} = {value}"))),
    );
    named
}

// Every case the old record spoke about that the new one will not. A disabled case is set aside
// and can come back, and its answer has to be measured again when it does.
fn name_what_the_corpus_no_longer_holds(
    corpus: &Corpus,
    held: Option<&RecordedAnswers>,
) -> Vec<(String, &'static str)> {
    let Some(held) = held else { return Vec::new() };
    let mut named: Vec<(String, &'static str)> = held
        .name_every_answer_block()
        .chain(held.name_every_exception_block())
        .filter(|(case, _)| !corpus.cases.iter().any(|one| one.name == *case))
        .map(|(case, second)| {
            let why = match corpus.disabled.iter().any(|one| one == case) {
                true => "dropped, the case is disabled",
                false => "dropped, no such case",
            };
            (format!("{case}.{second}"), why)
        })
        .collect();
    named.sort();
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
            note_is_inherited: false,
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
        write_the_answer(&mut text, &key, &recorded, &buckets, None).unwrap();

        let read: toml::Value = toml::from_str(&text)
            .unwrap_or_else(|e| panic!("what was written does not parse: {e}\n{text}"));
        let block = &read["answer"]["1010-a_case"]["default"];
        assert_eq!(block["counted"]["lines"].as_integer(), Some(4));
        assert_eq!(block["counted-regions"].as_array().map(Vec::len), Some(2));
        assert_eq!(block["is-known-failure"].as_bool(), Some(true));
        assert!(block["note"].as_str().unwrap().starts_with("the second"));
    }

    // The version-check workflow greps these phrases and takes the last field of each line.
    #[test]
    fn the_lines_the_version_check_reads_are_the_lines_this_writes() {
        let mut printed = Vec::new();
        write_what_moved(
            &mut printed,
            &["1010-a_case.default".to_string()],
            &["2010-another.stripstr".to_string()],
            &["3010-a_third.default".to_string()],
        )
        .unwrap();
        let printed = String::from_utf8(printed).unwrap();
        for wanted in ["note dropped", "started failing", "stopped failing"] {
            let line = printed
                .lines()
                .find(|line| line.contains(wanted))
                .unwrap_or_else(|| panic!("{wanted} is not printed\n{printed}"));
            let last = line.split_whitespace().next_back().unwrap();
            assert!(last.contains('.'), "{wanted} must end in <case>.<variation>, got {last}");
        }
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
        write_the_answer(&mut text, &key, &recorded, &["code".to_string()], None).unwrap();
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
