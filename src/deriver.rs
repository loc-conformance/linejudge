use std::collections::BTreeMap;

use crate::answer::{Answer, Counts, RegionCounts};
use crate::dialects::{Condition, Predicate, Rule, find_optional_readings, find_rules};
use crate::truth::{Truth, TruthLine};
use crate::truth::{COMMENT_MARKS, RESIDUE, STRING_MARKS, TAG_CLOSES, TAG_OPENS};

const DOC_STRING_SYMBOLS: [&str; 2] = ["\"\"\"", "'''"];
/// The section a dialect answers its optional readings in. The dialect files themselves arrive with
/// M7; the name is decided, and a refusal that names the section is worth more than one that does
/// not, so it is written here until there is a file to write it in.
const OPTIONAL_SECTION: &str = "counts-as-its-own-language";

/// The answer, and one true or false per rule of that counter saying whether the rule decided a
/// line of this file. The second is here because "is this rule ever used" is a question about all
/// the cases together, which no single file can answer.
pub struct Derivation {
    pub real: Answer,
    pub rules_that_fired: Vec<bool>,
}

/// The answer this counter should give for this file, worked out from its marked strings and
/// comments and from the counter's own rules. Two things are refused: a line that no rule puts
/// anywhere, and a line that two rules put in different buckets. The rules are not tried in order,
/// so there is no first one to win, and either case means the rules are wrong, not the file.
pub fn derive_answer(
    truth: &Truth,
    counter: &str,
    dialect: &str,
) -> Result<Derivation, Vec<String>> {
    let key = format!("{counter}.{dialect}");
    let Some(rules) = find_rules(counter, dialect) else {
        return Err(vec![format!("{key} is a dialect this suite has no rules for")]);
    };
    let answers = find_optional_readings(counter, dialect).unwrap_or_default();

    let mut faults = Vec::new();
    for reading in find_readings_of(truth) {
        if !answers.iter().any(|(named, _)| *named == reading) {
            faults.push(format!(
                "{key} does not say whether it counts {reading} as a language of its own, which \
                 this case marks as optional. Declare {reading} in the [{OPTIONAL_SECTION}] \
                 section of this dialect file and say how you want those lines counted"
            ));
        }
    }
    if !faults.is_empty() {
        return Err(faults);
    }
    let counted: Vec<&str> =
        answers.iter().filter(|(_, counts)| *counts).map(|(named, _)| *named).collect();

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
        return Err(faults);
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

/// One field per `Predicate`, answered for a single line by reading its characters together with
/// the marks under them.
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
}

fn judge_line<'r>(
    facts: &Facts,
    rules: &'r [Rule],
    rules_that_fired: &mut [bool],
) -> Result<&'r str, String> {
    let mut bucket: Option<&'r str> = None;
    for (index, rule) in rules.iter().enumerate() {
        if !rule.when.iter().all(|condition| facts.meets(condition)) {
            continue;
        }
        rules_that_fired[index] = true;
        match bucket {
            None => bucket = Some(rule.bucket),
            Some(already) if already == rule.bucket => {}
            Some(already) => {
                return Err(format!("two rules disagree, {already} against {}, on", rule.bucket));
            }
        }
    }
    bucket.ok_or_else(|| "no rule decides".to_string())
}

/// True or false for every line of the file. A doc string is a string that opens with three quotes
/// at the start of a line and lasts until that string closes, so a line in the middle of one holds
/// nothing that says so, and the only way to answer is to read the file from the top.
/// Every reading this file marks as optional, each named once, which is the set of questions a
/// dialect has to have answered before its answer for this file can be worked out.
fn find_readings_of(truth: &Truth) -> Vec<&str> {
    let mut found: Vec<&str> = truth
        .lines
        .iter()
        .flat_map(|line| line.regions.iter())
        .filter_map(|claim| claim.reading.as_deref())
        .collect();
    found.sort_unstable();
    found.dedup();
    found
}

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
    Counts { lines: 0, buckets: rules.iter().map(|rule| (rule.bucket.to_string(), 0)).collect() }
}

fn add_one_line(counts: &mut Counts, bucket: &str) {
    counts.lines += 1;
    *counts.buckets.entry(bucket.to_string()).or_default() += 1;
}

// The characters of a tag like <script> count as ordinary ones here. A tag decides which language
// the lines inside it belong to; it does not change what its own line is, and all three counters
// call that line code.
fn is_residue(mark: char) -> bool {
    matches!(mark, RESIDUE | TAG_OPENS | TAG_CLOSES)
}

fn is_word(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || !ch.is_ascii()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::corpus::Corpus;
    use crate::dialects::Condition::{Fails, Holds};
    use crate::dialects::Predicate::{Blank, HasResidue};
    use crate::dialects::find_every_dialect;

    use super::*;

    // A rule that no case reaches has never been checked against what the counter really does.
    #[test]
    fn every_rule_of_every_dialect_decides_a_line_of_the_corpus() {
        let corpus = read_the_corpus();
        let mut dead = Vec::new();
        for (counter, dialect) in find_every_dialect() {
            let rules = find_rules(counter, dialect).unwrap();
            let mut ever_fired = vec![false; rules.len()];
            for case in &corpus.cases {
                let found = derive_answer(&case.truth, counter, dialect).unwrap();
                for (ever, here) in ever_fired.iter_mut().zip(found.rules_that_fired) {
                    *ever |= here;
                }
            }
            for (index, fired) in ever_fired.iter().enumerate() {
                if !fired {
                    dead.push(format!("{counter}.{dialect} rule {} decides no line", index + 1));
                }
            }
        }
        assert!(dead.is_empty(), "{}", dead.join("\n"));
    }

    #[test]
    fn a_line_no_rule_decides_and_two_rules_that_disagree_are_both_refused() {
        let facts = Facts::of(&create_a_line("x = 1", ". . ."), false);

        let silent: &[Rule] = &[Rule { when: &[Holds(Blank)], bucket: "blanks" }];
        let refused = judge_line(&facts, silent, &mut [false]).unwrap_err();
        assert!(refused.contains("no rule decides"), "{refused}");

        let disagreeing: &[Rule] = &[
            Rule { when: &[Holds(HasResidue)], bucket: "code" },
            Rule { when: &[Fails(Blank)], bucket: "comments" },
        ];
        let refused = judge_line(&facts, disagreeing, &mut [false, false]).unwrap_err();
        assert!(refused.contains("code against comments"), "{refused}");
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
        let refused = derive_answer(&truth, "tokei", "default")
            .err()
            .unwrap_or_else(|| panic!("an unanswered reading was worked out anyway"));
        assert!(refused[0].contains("js-jsdoc"), "{refused:?}");
        assert!(refused[0].contains("[counts-as-its-own-language] section"), "{refused:?}");
    }

    #[test]
    fn a_dialect_that_declines_an_optional_region_charges_its_lines_to_the_one_around_it() {
        let input = "<script>\n/** doc */\nlet x = 1\n</script>\n";
        let marked = "<script>\n>>>>>>>> TypeScript\n/** doc */\n\
                      CCCcccccUU Markdown (optional rust-doc-comment)\n\
                      let x = 1\n... . . .\n</script>\n<<<<<<<<<\n";
        let truth = Truth::read(marked, input).unwrap();
        let charged = |counter, dialect| {
            derive_answer(&truth, counter, dialect)
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
    }

    #[test]
    fn a_doc_string_reads_as_comment_to_scc_alone_and_from_its_opener_to_its_closer() {
        let input = "\"\"\"\nnotes\n\"\"\"\nx = 1\n";
        let marked = "\"\"\"\nSSS\nnotes\nsssss\n\"\"\"\nZZZ\nx = 1\n. . .\n";
        let truth = Truth::read(marked, input).unwrap();
        let bucket = |counter, dialect, name: &str| {
            derive_answer(&truth, counter, dialect).unwrap().real.counts.buckets[name]
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
}
