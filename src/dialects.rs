use std::collections::BTreeMap;

use self::Condition::{Fails, Holds};
use self::Predicate::{
    Blank, HasResidue, InComment, InDocString, InString, WordInComment, WordInResidue,
};

/// A rule takes a line when all of its conditions hold, and puts that line in its bucket. The
/// rules are not tried in order and neither are the conditions: a rule's conditions say everything
/// about when it applies, and two rules taking the same line is allowed only where they name the
/// same bucket.
pub struct Rule {
    pub when: &'static [Condition],
    pub bucket: &'static str,
}

/// A question a rule asks, and whether it needs the answer to be yes or no.
pub enum Condition {
    Holds(Predicate),
    Fails(Predicate),
}

/// What a rule can ask about a line: about its own characters, about the strings and comments
/// covering them, and about a string or comment that opened on an earlier line and is still open
/// here. Residue means what is left of a line when its strings and comments are taken away, and a
/// word means a letter, a digit, or any character above ASCII.
#[derive(Clone, Copy, Debug)]
pub enum Predicate {
    Blank,
    HasResidue,
    InComment,
    /// The line is inside a string that opened with three quotes at the start of a line, spaces
    /// before them allowed. Only scc asks this.
    InDocString,
    InString,
    WordInComment,
    WordInResidue,
}

/// How many buckets a counter has is its own business. Three is what every counter here happens to
/// have, and the design leaves room for one that keeps a bucket of its own.
pub fn find_buckets(counter: &str, dialect: &str) -> Option<&'static [&'static str]> {
    find_dialect(counter, dialect).map(|found| found.buckets)
}

pub fn find_rules(counter: &str, dialect: &str) -> Option<&'static [Rule]> {
    find_dialect(counter, dialect).map(|found| found.rules)
}

/// What this way of counting says about each reading a case can mark as optional: `true` where it
/// counts that stretch as a language of its own, `false` where it leaves those lines to the code
/// around them. A reading missing from the list is a question this dialect has not answered, which
/// is refused rather than read as either answer.
pub fn find_optional_readings(
    counter: &str,
    dialect: &str,
) -> Option<&'static [(&'static str, bool)]> {
    find_dialect(counter, dialect).map(|found| found.optional_readings)
}

pub fn find_every_dialect() -> impl Iterator<Item = (&'static str, &'static str)> {
    DIALECTS.iter().map(|found| (found.counter, found.name))
}

pub fn check_buckets(found: &BTreeMap<String, u32>, wanted: &[&str]) -> Result<(), String> {
    for name in wanted {
        if !found.contains_key(*name) {
            return Err(format!("has no {name} bucket, and this dialect has {}", wanted.join(", ")));
        }
    }
    for name in found.keys() {
        if !wanted.contains(&name.as_str()) {
            return Err(format!(
                "has a bucket named {name}, and this dialect has {}",
                wanted.join(", ")
            ));
        }
    }
    Ok(())
}

struct Dialect {
    counter: &'static str,
    name: &'static str,
    buckets: &'static [&'static str],
    rules: &'static [Rule],
    optional_readings: &'static [(&'static str, bool)],
}

/// Every counter this suite knows and every way it has of counting: the names it gives its
/// buckets, the rules that put each line in one of them, and its answer to each reading a case can
/// mark as optional. Case files, adapter files and what a counter prints are all checked against
/// this table.
///
/// Every dialect answers every reading for itself, three of them saying the same thing today. A
/// shared list would let one edit answer a new question on behalf of four counters, and the answer
/// is a measurement of each.
const DIALECTS: [Dialect; 4] = [
    Dialect {
        counter: "mezura",
        name: "content",
        buckets: &["code", "comments", "extra"],
        rules: CONTENT_RULES,
        optional_readings: &[("rust-doc-comment", false), ("vue-template", false)],
    },
    Dialect {
        counter: "mezura",
        name: "region",
        buckets: &["code", "comments", "blanks"],
        rules: SHARED_RULES,
        optional_readings: &[("rust-doc-comment", false), ("vue-template", false)],
    },
    Dialect {
        counter: "scc",
        name: "default",
        buckets: &["code", "comments", "blanks"],
        rules: SCC_RULES,
        optional_readings: &[("rust-doc-comment", false), ("vue-template", false)],
    },
    Dialect {
        counter: "tokei",
        name: "default",
        buckets: &["code", "comments", "blanks"],
        rules: SHARED_RULES,
        optional_readings: &[("rust-doc-comment", true), ("vue-template", true)],
    },
];

const CONTENT_RULES: &[Rule] = &[
    Rule { when: &[Holds(WordInResidue)], bucket: "code" },
    Rule { when: &[Holds(InString), Fails(Blank), Fails(WordInResidue)], bucket: "code" },
    // The last condition is what keeps a line like `"s" /* words */` from matching this rule and
    // the string rule above it with two different buckets. mezura looks at a line's string before
    // its comment, and since rules have no order, that preference has to be written as a condition.
    Rule {
        when: &[Holds(WordInComment), Fails(WordInResidue), Fails(InString)],
        bucket: "comments",
    },
    Rule { when: &[Holds(Blank)], bucket: "extra" },
    Rule {
        when: &[Fails(Blank), Fails(InString), Fails(WordInResidue), Fails(WordInComment)],
        bucket: "extra",
    },
];

const SCC_RULES: &[Rule] = &[
    Rule { when: &[Holds(HasResidue)], bucket: "code" },
    Rule { when: &[Holds(InString), Fails(InDocString)], bucket: "code" },
    // scc calls the last line of a doc string code when something else sits on it, `""" + x`, so
    // that line has to fall through to the first rule. That is why this rule asks for nothing
    // outside the string, and why the first rule does not have to ask about doc strings at all.
    Rule { when: &[Holds(InDocString), Fails(HasResidue)], bucket: "comments" },
    Rule { when: &[Holds(InComment), Fails(HasResidue), Fails(InString)], bucket: "comments" },
    Rule { when: &[Holds(Blank), Fails(InComment), Fails(InString)], bucket: "blanks" },
];

const SHARED_RULES: &[Rule] = &[
    Rule { when: &[Holds(HasResidue)], bucket: "code" },
    Rule { when: &[Holds(InString)], bucket: "code" },
    Rule { when: &[Holds(InComment), Fails(HasResidue), Fails(InString)], bucket: "comments" },
    Rule { when: &[Holds(Blank), Fails(InComment), Fails(InString)], bucket: "blanks" },
];

fn find_dialect(counter: &str, dialect: &str) -> Option<&'static Dialect> {
    DIALECTS.iter().find(|found| found.counter == counter && found.name == dialect)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dialect_off_the_roster_has_no_buckets_and_no_rules() {
        assert_eq!(find_buckets("mezura", "content").unwrap()[2], "extra");
        assert_eq!(find_buckets("tokei", "default").unwrap()[2], "blanks");
        assert!(find_buckets("tokei", "strict").is_none());
        assert!(find_rules("tokei", "strict").is_none());
        assert!(find_buckets("cloc", "default").is_none());
        assert_eq!(find_every_dialect().count(), 4);
    }

    // Every dialect answers every reading, whatever the answer is, because a question left out is
    // a case this suite cannot work out an answer for.
    #[test]
    fn every_dialect_answers_every_reading_and_tokei_is_the_one_that_counts_them() {
        let asked: Vec<&str> = find_optional_readings("tokei", "default")
            .unwrap()
            .iter()
            .map(|(reading, _)| *reading)
            .collect();
        assert_eq!(asked, ["rust-doc-comment", "vue-template"]);
        for (counter, dialect) in find_every_dialect() {
            let answers = find_optional_readings(counter, dialect).unwrap();
            let named: Vec<&str> = answers.iter().map(|(reading, _)| *reading).collect();
            assert_eq!(named, asked, "{counter}.{dialect}");
            let counted = answers.iter().all(|(_, counts)| *counts);
            assert_eq!(counted, counter == "tokei", "{counter}.{dialect}");
        }
        assert!(find_optional_readings("cloc", "default").is_none());
    }

    // Both directions matter. A rule naming a bucket that is not in the list would count lines
    // into a fourth one nobody reads, and a bucket that no rule names can never be filled, so the
    // answer would come out missing a column that every case file has.
    #[test]
    fn the_buckets_of_a_dialect_are_exactly_the_ones_its_rules_name() {
        for found in &DIALECTS {
            let mut named: Vec<&str> = found.rules.iter().map(|rule| rule.bucket).collect();
            named.sort_unstable();
            named.dedup();
            let mut has: Vec<&str> = found.buckets.to_vec();
            has.sort_unstable();
            assert_eq!(named, has, "{}.{}", found.counter, found.name);
        }
    }

    #[test]
    fn a_bucket_set_is_exactly_the_dialects_own_and_says_which_when_it_is_not() {
        let wanted = ["code", "comments", "blanks"];
        let named = |names: [(&str, u32); 3]| {
            names.iter().map(|(n, v)| (n.to_string(), *v)).collect::<BTreeMap<_, _>>()
        };
        assert!(check_buckets(&named([("code", 1), ("comments", 1), ("blanks", 0)]), &wanted).is_ok());
        let missing = check_buckets(&named([("code", 1), ("comments", 1), ("extra", 0)]), &wanted);
        assert!(missing.unwrap_err().contains("no blanks bucket"));
    }
}
