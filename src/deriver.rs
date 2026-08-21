//! Works out the answer a counter should give, from a case's marks and that counter's rules.

use std::collections::BTreeMap;

use crate::answer::{Answer, Counts, RegionCounts};
use crate::dialects::{Condition, Dialect, OPTIONAL_SECTION, PREDICATES, Predicate, Rule};
use crate::faults::Faults;
use crate::readings::Readings;
use crate::truth::{Truth, TruthLine};
use crate::truth::{COMMENT_MARKS, RESIDUE, STRING_MARKS, TAG_CLOSES, TAG_OPENS};

const DOC_STRING_SYMBOLS: [&str; 2] = ["\"\"\"", "'''"];

/// What one dialect makes of one case.
pub struct Derivation {
    /// The answer the counter should give.
    pub real: Answer,
    /// One entry per rule of the dialect, saying whether it took a line of this file. Whether a
    /// rule is ever used at all is a question about the whole corpus, so the tally is handed back
    /// to be summed.
    pub rules_that_fired: Vec<bool>,
}

/// Works out the answer this counter should give for this file. Two things are refused: a line no
/// rule puts anywhere, and a line two rules put in different buckets. The rules are not tried in
/// order, so there is no first one to win, and either case means the rules are wrong, not the file.
pub fn derive_answer(
    truth: &Truth,
    dialect: &Dialect,
    readings: &Readings,
) -> Result<Derivation, Faults> {
    let key = format!("{}.{}", dialect.counter, dialect.name);
    let mut faults = check_readings_are_answered(truth, dialect, readings);
    if !faults.is_empty() {
        return Err(faults.into());
    }
    let counted = collect_counted_readings(dialect);

    let rules = &dialect.rules;
    let mut counts = create_empty_counts(rules);
    let mut regions: BTreeMap<&str, Counts> = BTreeMap::new();
    let mut rules_that_fired = vec![false; rules.len()];
    for (line, in_doc_string) in truth.lines.iter().zip(find_lines_in_a_doc_string(truth)) {
        let facts = Facts::of(line, in_doc_string);
        let bucket = match judge_line(&facts, rules, &mut rules_that_fired) {
            Ok(bucket) => bucket,
            Err(message) => {
                faults.push(format!("{key}: {message} [{}]", line.source));
                continue;
            }
        };
        add_one_line(&mut counts, bucket);
        if let Some(claim) = line.find_region(&counted) {
            let region = regions
                .entry(claim.language.as_str())
                .or_insert_with(|| create_empty_counts(rules));
            add_one_line(region, bucket);
        }
    }
    if !faults.is_empty() {
        return Err(faults.into());
    }

    let regions = regions
        .into_iter()
        .map(|(language, counts)| RegionCounts {
            language: language.to_string(),
            lines: counts.lines,
            buckets: counts.buckets,
        })
        .collect();
    Ok(Derivation { real: Answer { counts, regions }, rules_that_fired })
}

/// One line of the file as this dialect's rules read it, for a person asking why the counts came
/// out the way they did.
pub struct ExplainedLine {
    pub bucket: String,
    /// The rules that took the line, two names where two rules agree on it.
    pub rules: Vec<String>,
    /// The predicates that hold on the line; one that is not named does not hold.
    pub holds: Vec<&'static str>,
    /// The language this line counts towards, and `None` is the file itself.
    pub region: Option<String>,
}

/// The same work as [`derive_answer`], kept line by line instead of summed, for a person asking
/// why the counts came out the way they did. Both go through one function, so the two cannot part.
///
/// One entry per line of the input, in the file's own order, so it lines up with
/// [`Truth::lines`](crate::truth::Truth::lines). A line no rule can place is an `Err` and never a
/// shorter list.
pub fn explain_every_line(
    truth: &Truth,
    dialect: &Dialect,
    readings: &Readings,
) -> Result<Vec<ExplainedLine>, Faults> {
    let key = format!("{}.{}", dialect.counter, dialect.name);
    let faults = check_readings_are_answered(truth, dialect, readings);
    if !faults.is_empty() {
        return Err(faults.into());
    }
    let counted = collect_counted_readings(dialect);

    let rules = &dialect.rules;
    let mut lines = Vec::with_capacity(truth.lines.len());
    let mut faults = Vec::new();
    for (line, in_doc_string) in truth.lines.iter().zip(find_lines_in_a_doc_string(truth)) {
        let facts = Facts::of(line, in_doc_string);
        let mut fired = vec![false; rules.len()];
        let bucket = match judge_line(&facts, rules, &mut fired) {
            Ok(bucket) => bucket.to_string(),
            Err(message) => {
                faults.push(format!("{key}: {message} [{}]", line.source));
                continue;
            }
        };
        lines.push(ExplainedLine {
            bucket,
            rules: rules
                .iter()
                .zip(&fired)
                .filter(|(_, fired)| **fired)
                .map(|(rule, _)| rule.name.clone())
                .collect(),
            holds: facts.find_predicates_that_hold(),
            region: line.find_region(&counted).map(|claim| claim.language.clone()),
        });
    }
    if faults.is_empty() { Ok(lines) } else { Err(faults.into()) }
}

// One field per `Predicate`, answered for a single line.
struct Facts {
    blank: bool,
    has_residue: bool,
    in_comment: bool,
    in_doc_string: bool,
    in_string: bool,
    word_in_comment: bool,
    word_in_residue: bool,
}

impl Facts {
    fn of(line: &TruthLine, in_doc_string: bool) -> Facts {
        let columns = || line.source.chars().zip(line.marker.chars());
        Facts {
            blank: line.source.chars().all(|ch| ch.is_ascii_whitespace()),
            has_residue: line.marker.chars().any(is_residue),
            in_comment: line.marker.chars().any(|mark| COMMENT_MARKS.owns(mark)),
            in_doc_string,
            in_string: line.marker.chars().any(|mark| STRING_MARKS.owns(mark)),
            word_in_comment: columns().any(|(ch, mark)| COMMENT_MARKS.owns(mark) && is_word(ch)),
            word_in_residue: columns().any(|(ch, mark)| is_residue(mark) && is_word(ch)),
        }
    }

    fn meets(&self, condition: &Condition) -> bool {
        match condition {
            Condition::Holds(predicate) => self.holds(*predicate),
            Condition::Fails(predicate) => !self.holds(*predicate),
        }
    }

    fn holds(&self, predicate: Predicate) -> bool {
        match predicate {
            Predicate::Blank => self.blank,
            Predicate::HasResidue => self.has_residue,
            Predicate::InComment => self.in_comment,
            Predicate::InDocString => self.in_doc_string,
            Predicate::InString => self.in_string,
            Predicate::WordInComment => self.word_in_comment,
            Predicate::WordInResidue => self.word_in_residue,
        }
    }

    fn find_predicates_that_hold(&self) -> Vec<&'static str> {
        PREDICATES
            .iter()
            .filter(|(_, predicate)| self.holds(*predicate))
            .map(|(name, _)| *name)
            .collect()
    }
}

fn judge_line<'r>(
    facts: &Facts,
    rules: &'r [Rule],
    rules_that_fired: &mut [bool],
) -> Result<&'r str, String> {
    let mut took: Option<&'r Rule> = None;
    for (index, rule) in rules.iter().enumerate() {
        if !rule.when.iter().all(|condition| facts.meets(condition)) {
            continue;
        }
        rules_that_fired[index] = true;
        match took {
            None => took = Some(rule),
            Some(already) if already.bucket == rule.bucket => {}
            Some(already) => {
                return Err(format!(
                    "two rules disagree, {} ({}) against {} ({}), on",
                    already.bucket, already.name, rule.bucket, rule.name
                ));
            }
        }
    }
    took.map(|rule| rule.bucket.as_str()).ok_or_else(|| "no rule decides".to_string())
}

fn check_readings_are_answered(
    truth: &Truth,
    dialect: &Dialect,
    readings: &Readings,
) -> Vec<String> {
    let key = format!("{}.{}", dialect.counter, dialect.name);
    let mut faults = Vec::new();
    for reading in truth.find_optional_readings() {
        if !dialect.optional_readings.contains_key(reading) {
            let explained = readings
                .find(reading)
                .map(|found| format!(". {reading} is {}", found.sentence))
                .unwrap_or_default();
            faults.push(format!(
                "{key} does not say whether it counts {reading} as a language of its own, which \
                 this case marks as optional. Declare {reading} in the [{OPTIONAL_SECTION}] \
                 section of {} and say how you want those lines counted{explained}",
                dialect.file.display()
            ));
        }
    }
    faults
}

fn collect_counted_readings(dialect: &Dialect) -> Vec<&str> {
    dialect
        .optional_readings
        .iter()
        .filter(|(_, counts)| **counts)
        .map(|(named, _)| named.as_str())
        .collect()
}

// A line in the middle of a doc string holds nothing saying it is in one, so the whole file has to
// be read from the top and the answer kept per line.
fn find_lines_in_a_doc_string(truth: &Truth) -> Vec<bool> {
    let mut open = false;
    let mut lines = Vec::with_capacity(truth.lines.len());
    for line in &truth.lines {
        if open && !line.marker.starts_with([STRING_MARKS.interior, STRING_MARKS.closer]) {
            open = false;
        }
        let mut on_this_line = open;
        let columns: Vec<(char, char)> = line.source.chars().zip(line.marker.chars()).collect();
        let mut at = 0;
        while at < columns.len() {
            let start = at;
            let mark = columns[at].1;
            while at < columns.len() && columns[at].1 == mark {
                at += 1;
            }
            if mark == STRING_MARKS.opener {
                let symbol: String = columns[start..at].iter().map(|(ch, _)| *ch).collect();
                let nothing_before =
                    columns[..start].iter().all(|(ch, _)| ch.is_ascii_whitespace());
                open = nothing_before && DOC_STRING_SYMBOLS.contains(&symbol.as_str());
            } else if mark != STRING_MARKS.interior && mark != STRING_MARKS.closer {
                open = false;
            }
            on_this_line |= open;
            if mark == STRING_MARKS.closer {
                open = false;
            }
        }
        lines.push(on_this_line);
    }
    lines
}

fn create_empty_counts(rules: &[Rule]) -> Counts {
    Counts { lines: 0, buckets: rules.iter().map(|rule| (rule.bucket.clone(), 0)).collect() }
}

fn add_one_line(counts: &mut Counts, bucket: &str) {
    counts.lines += 1;
    *counts.buckets.entry(bucket.to_string()).or_default() += 1;
}

// A tag like <script> decides which language the lines inside it belong to, and does not change
// what its own line is: all three counters call that line code.
fn is_residue(mark: char) -> bool {
    matches!(mark, RESIDUE | TAG_OPENS | TAG_CLOSES)
}

fn is_word(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || !ch.is_ascii()
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::fs;
    use std::path::Path;

    use crate::corpus::Corpus;
    use crate::dialects::Condition::{Fails, Holds};
    use crate::dialects::Predicate::{Blank, HasResidue};
    use crate::dialects::read_the_shipped_dialects;
    use crate::readings::read_the_shipped_readings;

    use super::*;

    // A rule that no case reaches has never been checked against what the counter really does.
    #[test]
    fn every_rule_of_every_dialect_decides_a_line_of_the_corpus() {
        let dialects = read_the_shipped_dialects();
        let readings = read_the_shipped_readings();
        let corpus = read_the_corpus();
        let mut dead = Vec::new();
        for dialect in dialects.iter() {
            let mut ever_fired = vec![false; dialect.rules.len()];
            for case in &corpus.cases {
                let found = derive_answer(&case.truth, dialect, &readings).unwrap();
                for (ever, here) in ever_fired.iter_mut().zip(found.rules_that_fired) {
                    *ever |= here;
                }
            }
            for (rule, fired) in dialect.rules.iter().zip(ever_fired) {
                if !fired {
                    dead.push(format!(
                        "{}.{} rule {} decides no line",
                        dialect.counter, dialect.name, rule.name
                    ));
                }
            }
        }
        assert!(dead.is_empty(), "{}", dead.join("\n"));
    }

    #[test]
    fn a_line_no_rule_decides_and_two_rules_that_disagree_are_both_refused() {
        let facts = Facts::of(&create_a_line("x = 1", ". . ."), false);
        let rule = |name: &str, when: Vec<Condition>, bucket: &str| Rule {
            name: name.to_string(),
            when,
            bucket: bucket.to_string(),
        };

        let silent = [rule("blank-line", vec![Holds(Blank)], "blanks")];
        let refused = judge_line(&facts, &silent, &mut [false]).unwrap_err();
        assert!(refused.contains("no rule decides"), "{refused}");

        let disagreeing = [
            rule("residue", vec![Holds(HasResidue)], "code"),
            rule("not-blank", vec![Fails(Blank)], "comments"),
        ];
        let refused = judge_line(&facts, &disagreeing, &mut [false, false]).unwrap_err();
        assert!(refused.contains("code (residue) against comments (not-blank)"), "{refused}");
    }

    // No file in the corpus holds a character above ASCII, so this is the only place that tests
    // what happens to one.
    #[test]
    fn a_word_is_a_letter_a_digit_or_anything_above_ascii_and_a_column_is_a_character() {
        let facts = |source: &str, marker: &str| Facts::of(&create_a_line(source, marker), false);
        assert!(!facts("/* ... */", "CCcccccUU").word_in_comment);
        assert!(facts("/* ναι */", "CCcccccUU").word_in_comment);
        assert!(!facts("{ } ;", ". . .").word_in_residue);
        assert!(facts("{ x ;", ". . .").word_in_residue);
    }

    // The reading a dialect has never answered is the one a new kind of case brings, and the
    // answer cannot be guessed: reading it as a no would blame the counter for our omission.
    #[test]
    fn a_reading_no_dialect_has_answered_is_refused_and_says_what_to_do_about_it() {
        let input = "/** doc */\nlet x = 1\n";
        let marked = "/** doc */\nCCCcccccUU Markdown (optional js-jsdoc)\nlet x = 1\n... . . .\n";
        let truth = Truth::read(marked, input).unwrap();
        let dialects = read_the_shipped_dialects();
        let readings = create_readings_that_define(
            "[js-jsdoc]\nsentence = \"the body of a JSDoc comment\"\nwitness = \"0100-a_case\"\n",
        );
        let refused = derive_answer(&truth, dialects.find("tokei", "default").unwrap(), &readings)
            .err()
            .unwrap_or_else(|| panic!("an unanswered reading was worked out anyway"));
        assert!(refused[0].contains("js-jsdoc"), "{refused:?}");
        assert!(refused[0].contains("[counts-as-its-own-language] section"), "{refused:?}");
        assert!(refused[0].contains("default.toml"), "{refused:?}");
        assert!(refused[0].contains("js-jsdoc is the body of a JSDoc comment"), "{refused:?}");
    }

    #[test]
    fn a_dialect_that_declines_an_optional_region_charges_its_lines_to_the_one_around_it() {
        let input = "<script>\n/** doc */\nlet x = 1\n</script>\n";
        let marked = "<script>\n>>>>>>>> TypeScript\n/** doc */\n\
                      CCCcccccUU Markdown (optional rust-doc-comment)\n\
                      let x = 1\n... . . .\n</script>\n<<<<<<<<<\n";
        let truth = Truth::read(marked, input).unwrap();
        let dialects = read_the_shipped_dialects();
        let readings = read_the_shipped_readings();
        let charged = |counter, dialect| {
            derive_answer(&truth, dialects.find(counter, dialect).unwrap(), &readings)
                .unwrap()
                .real
                .regions
                .iter()
                .map(|region| (region.language.clone(), region.lines))
                .collect::<Vec<(String, u32)>>()
        };
        let named = |language: &str, lines| (language.to_string(), lines);
        assert_eq!(charged("tokei", "default"), [named("Markdown", 1), named("TypeScript", 1)]);
        assert_eq!(charged("mezura", "region"), [named("TypeScript", 2)]);

        let doc_line_of = |counter, dialect| {
            explain_every_line(&truth, dialects.find(counter, dialect).unwrap(), &readings)
                .unwrap()
                .remove(1)
                .region
        };
        assert_eq!(doc_line_of("tokei", "default").as_deref(), Some("Markdown"));
        assert_eq!(doc_line_of("mezura", "region").as_deref(), Some("TypeScript"));
    }

    #[test]
    fn every_line_is_explained_by_the_same_rules_that_count_it() {
        let input = "\"\"\"\nnotes\n\"\"\"\nx = 1\n";
        let marked = "\"\"\"\nSSS\nnotes\nsssss\n\"\"\"\nZZZ\nx = 1\n. . .\n";
        let truth = Truth::read(marked, input).unwrap();
        let dialects = read_the_shipped_dialects();
        let readings = read_the_shipped_readings();
        let scc = dialects.find("scc", "default").unwrap();

        let explained = explain_every_line(&truth, scc, &readings).unwrap();
        assert_eq!(explained[0].bucket, "comments");
        assert_eq!(explained[0].rules, ["a-doc-string-is-documentation"]);
        assert!(explained[0].holds.contains(&"in-doc-string"), "{:?}", explained[0].holds);
        assert_eq!(explained[3].bucket, "code");
        assert_eq!(explained[3].rules, ["anything-outside-spans-is-code"]);
        assert_eq!(explained[3].region, None);

        let counted = derive_answer(&truth, scc, &readings).unwrap().real.counts;
        for (name, value) in &counted.buckets {
            let of = explained.iter().filter(|line| &line.bucket == name).count();
            assert_eq!(of as u32, *value, "{name}");
        }
    }

    #[test]
    fn a_doc_string_reads_as_comment_to_scc_alone_and_from_its_opener_to_its_closer() {
        let input = "\"\"\"\nnotes\n\"\"\"\nx = 1\n";
        let marked = "\"\"\"\nSSS\nnotes\nsssss\n\"\"\"\nZZZ\nx = 1\n. . .\n";
        let truth = Truth::read(marked, input).unwrap();
        let dialects = read_the_shipped_dialects();
        let readings = read_the_shipped_readings();
        let bucket = |counter, dialect, name: &str| {
            derive_answer(&truth, dialects.find(counter, dialect).unwrap(), &readings)
                .unwrap()
                .real
                .counts
                .buckets[name]
        };
        assert_eq!(bucket("scc", "default", "comments"), 3);
        assert_eq!(bucket("scc", "default", "code"), 1);
        assert_eq!(bucket("tokei", "default", "comments"), 0);
        assert_eq!(bucket("tokei", "default", "code"), 4);
    }

    fn read_the_corpus() -> Corpus {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("cases");
        Corpus::read(&dir).unwrap_or_else(|faults| {
            let report: Vec<String> = faults.iter().map(|fault| fault.to_string()).collect();
            panic!("{}", report.join("\n"))
        })
    }

    fn create_a_line(source: &str, marker: &str) -> TruthLine {
        TruthLine { source: source.to_string(), marker: marker.to_string(), regions: Vec::new() }
    }

    fn create_readings_that_define(text: &str) -> Readings {
        let dir = env::temp_dir().join("linejudge-a_reading_with_a_sentence");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("readings.toml"), text).unwrap();
        let readings = Readings::read(&dir).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        readings
    }
}
