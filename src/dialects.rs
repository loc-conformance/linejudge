use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

const DIALECT_EXTENSION: &str = "toml";
pub const OPTIONAL_SECTION: &str = "counts-as-its-own-language";
pub const PREDICATES: [(&str, Predicate); 7] = [
    ("blank", Predicate::Blank),
    ("has-residue", Predicate::HasResidue),
    ("in-comment", Predicate::InComment),
    ("in-doc-string", Predicate::InDocString),
    ("in-string", Predicate::InString),
    ("word-in-comment", Predicate::WordInComment),
    ("word-in-residue", Predicate::WordInResidue),
];

/// Every way of counting this run knows: one dialect per file in the directory it was read from.
/// Case files, adapter files and what a counter prints are all checked against these.
pub struct Dialects {
    dialects: Vec<Dialect>,
}

impl Dialects {
    /// One folder per counter, one file per way it counts: `<counter>/<dialect>.toml`. The folder
    /// is the unit a tool's own maintainer owns, however many dialects it grows.
    pub fn read(dir: &Path) -> Result<Dialects, Vec<String>> {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) => {
                return Err(vec![format!("{} could not be opened: {error}", dir.display())]);
            }
        };
        let mut folders = Vec::new();
        let mut faults = Vec::new();
        for path in entries.filter_map(|e| e.ok()).map(|e| e.path()) {
            if path.is_dir() {
                folders.push(path);
            } else if path.extension().is_some_and(|e| e == DIALECT_EXTENSION) {
                faults.push(format!(
                    "{} sits at the top of the dialects directory, and a dialect lives in its \
                     counter's own folder, <counter>/<dialect>.toml",
                    path.display()
                ));
            }
        }
        folders.sort();

        let mut dialects = Vec::new();
        for folder in folders {
            let entries = match fs::read_dir(&folder) {
                Ok(entries) => entries,
                Err(error) => {
                    faults.push(format!("{} could not be opened: {error}", folder.display()));
                    continue;
                }
            };
            let mut paths: Vec<PathBuf> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == DIALECT_EXTENSION))
                .collect();
            paths.sort();
            for path in paths {
                match Dialect::read(&path) {
                    Ok(dialect) => dialects.push(dialect),
                    Err(mut found) => faults.append(&mut found),
                }
            }
        }
        if dialects.is_empty() && faults.is_empty() {
            faults.push(format!("{} holds no dialect file", dir.display()));
        }
        if faults.is_empty() { Ok(Dialects { dialects }) } else { Err(faults) }
    }

    pub fn find(&self, counter: &str, dialect: &str) -> Option<&Dialect> {
        self.dialects.iter().find(|d| d.counter == counter && d.name == dialect)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Dialect> {
        self.dialects.iter()
    }
}

/// One way of counting, as its file declares it: the names it gives its buckets, the rules that
/// put each line in one of them, and its answer to each reading a case can mark as optional.
pub struct Dialect {
    pub counter: String,
    pub name: String,
    /// The file this dialect was read from, so a refusal can say where the missing answer goes.
    pub file: PathBuf,
    pub buckets: Vec<String>,
    pub rules: Vec<Rule>,
    /// What this way of counting says about each reading a case can mark as optional: `true` where
    /// it counts that stretch as a language of its own, `false` where it leaves those lines to the
    /// code around them. A reading missing from the map is a question this dialect has not
    /// answered, which is refused rather than read as either answer.
    pub optional_readings: BTreeMap<String, bool>,
}

impl Dialect {
    /// Everything a dialect file is held to is decided here, and a file that breaks more than one
    /// rule says so once for each.
    pub fn read(path: &Path) -> Result<Dialect, Vec<String>> {
        let text = fs::read_to_string(path)
            .map_err(|e| vec![format!("{} could not be read: {e}", path.display())])?;
        let raw: RawDialect = toml::from_str(&text)
            .map_err(|e| vec![format!("{} does not parse: {e}", path.display())])?;
        let where_it_is = path.display();
        let mut faults = Vec::new();

        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let folder = path
            .parent()
            .and_then(|parent| parent.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let key = format!("{}.{}", raw.counter, raw.dialect);
        if folder != raw.counter || stem != raw.dialect {
            faults.push(format!(
                "{where_it_is} declares {key}, and a dialect is the file named after it, \
                 {}/{}.toml",
                raw.counter, raw.dialect
            ));
        }
        for pair in raw.buckets.windows(2) {
            if pair[0] == pair[1] {
                faults.push(format!("{where_it_is} names the {} bucket twice", pair[0]));
            }
        }
        if raw.rule.is_empty() {
            faults.push(format!("{where_it_is} holds no rule, so no line could ever be counted"));
        }

        let mut rules: Vec<Rule> = Vec::new();
        for raw_rule in raw.rule {
            let name = raw_rule.name;
            if name.trim().is_empty() {
                faults.push(format!("{where_it_is} holds a rule with no name"));
            }
            if rules.iter().any(|rule| rule.name == name) {
                faults.push(format!("{where_it_is} holds two rules both named {name}"));
            }
            let mut when = Vec::new();
            for token in &raw_rule.when {
                match parse_condition(token) {
                    Ok(condition) => when.push(condition),
                    Err(message) => {
                        faults.push(format!("{where_it_is}, rule {name}: {message}"));
                    }
                }
            }
            if !raw.buckets.contains(&raw_rule.bucket) {
                faults.push(format!(
                    "{where_it_is}, rule {name}: counts into {}, and the buckets are {}",
                    raw_rule.bucket,
                    raw.buckets.join(", ")
                ));
            }
            rules.push(Rule { name, when, bucket: raw_rule.bucket });
        }
        for bucket in &raw.buckets {
            if !rules.iter().any(|rule| &rule.bucket == bucket) {
                faults.push(format!(
                    "{where_it_is}: no rule counts into {bucket}, so it could never hold a line"
                ));
            }
        }

        if !faults.is_empty() {
            return Err(faults);
        }
        Ok(Dialect {
            counter: raw.counter,
            name: raw.dialect,
            file: path.to_path_buf(),
            buckets: raw.buckets,
            rules,
            optional_readings: raw.optional_readings,
        })
    }
}

/// A rule takes a line when all of its conditions hold, and puts that line in its bucket. The
/// rules are not tried in order and neither are the conditions: a rule's conditions say everything
/// about when it applies, and two rules taking the same line is allowed only where they name the
/// same bucket.
pub struct Rule {
    pub name: String,
    pub when: Vec<Condition>,
    pub bucket: String,
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

pub fn check_buckets(found: &BTreeMap<String, u32>, wanted: &[String]) -> Result<(), String> {
    for name in wanted {
        if !found.contains_key(name) {
            return Err(format!("has no {name} bucket, and this dialect has {}", wanted.join(", ")));
        }
    }
    for name in found.keys() {
        if !wanted.contains(name) {
            return Err(format!(
                "has a bucket named {name}, and this dialect has {}",
                wanted.join(", ")
            ));
        }
    }
    Ok(())
}

fn parse_condition(token: &str) -> Result<Condition, String> {
    let (bare, negated) = match token.strip_prefix('!') {
        Some(rest) => (rest, true),
        None => (token, false),
    };
    let Some((_, predicate)) = PREDICATES.iter().find(|(name, _)| *name == bare) else {
        let known: Vec<&str> = PREDICATES.iter().map(|(name, _)| *name).collect();
        return Err(format!("{token} is not a predicate; the predicates are {}", known.join(", ")));
    };
    Ok(if negated { Condition::Fails(*predicate) } else { Condition::Holds(*predicate) })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDialect {
    counter: String,
    dialect: String,
    buckets: Vec<String>,
    #[serde(rename = "counts-as-its-own-language", default)]
    optional_readings: BTreeMap<String, bool>,
    rule: Vec<RawRule>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRule {
    name: String,
    when: Vec<String>,
    bucket: String,
}

#[cfg(test)]
pub fn read_the_shipped_dialects() -> Dialects {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("dialects");
    Dialects::read(&dir).unwrap_or_else(|faults| panic!("{}", faults.join("\n")))
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    const ONE_DIALECT: &str = "counter = \"tokei\"\ndialect = \"default\"\n\
                               buckets = [\"code\", \"blanks\"]\n\
                               [[rule]]\nname = \"anything-at-all\"\nwhen = [\"!blank\"]\n\
                               bucket = \"code\"\n\
                               [[rule]]\nname = \"blank-line\"\nwhen = [\"blank\"]\n\
                               bucket = \"blanks\"\n";

    #[test]
    fn the_shipped_dialects_are_read_and_a_dialect_off_the_roster_is_not_there() {
        let dialects = read_the_shipped_dialects();
        assert_eq!(dialects.iter().count(), 4);
        let content = dialects.find("mezura", "content").unwrap();
        assert_eq!(content.buckets, ["code", "comments", "extra"]);
        assert_eq!(content.rules.len(), 5);
        assert_eq!(content.rules[0].name, "words-outside-spans-are-code");
        assert_eq!(dialects.find("tokei", "default").unwrap().buckets[2], "blanks");
        assert!(dialects.find("tokei", "strict").is_none());
        assert!(dialects.find("cloc", "default").is_none());
    }

    // Every dialect answers every reading, whatever the answer is, because a question left out is
    // a case this suite cannot work out an answer for.
    #[test]
    fn every_shipped_dialect_answers_every_reading_and_tokei_is_the_one_that_counts_them() {
        let dialects = read_the_shipped_dialects();
        let asked = ["rust-doc-comment", "vue-template"];
        for dialect in dialects.iter() {
            let key = format!("{}.{}", dialect.counter, dialect.name);
            let named: Vec<&str> =
                dialect.optional_readings.keys().map(String::as_str).collect();
            assert_eq!(named, asked, "{key}");
            let counted = dialect.optional_readings.values().all(|counts| *counts);
            assert_eq!(counted, dialect.counter == "tokei", "{key}");
        }
    }

    #[test]
    fn a_dialect_under_a_name_that_is_not_its_own_is_refused() {
        let refused = read_a_broken_dialect(
            "a_dialect_under_the_wrong_name",
            &ONE_DIALECT.replace("dialect = \"default\"", "dialect = \"strict\""),
        );
        assert!(refused[0].contains("declares tokei.strict"), "{refused:?}");
        assert!(refused[0].contains("named after it, tokei/strict.toml"), "{refused:?}");

        let wrong_folder = read_a_broken_dialect(
            "a_dialect_in_the_wrong_folder",
            &ONE_DIALECT.replace("counter = \"tokei\"", "counter = \"scc\""),
        );
        assert!(wrong_folder[0].contains("named after it, scc/default.toml"), "{wrong_folder:?}");
    }

    #[test]
    fn a_condition_that_asks_no_known_question_is_refused_beside_the_seven_that_exist() {
        let refused = read_a_broken_dialect(
            "a_dialect_asking_an_unknown_question",
            &ONE_DIALECT.replace("[\"blank\"]", "[\"!in-word\"]"),
        );
        assert!(refused[0].contains("rule blank-line: !in-word is not a predicate"), "{refused:?}");
        assert!(refused[0].contains("blank, has-residue, in-comment"), "{refused:?}");
    }

    #[test]
    fn two_rules_under_one_name_and_a_rule_with_no_name_are_both_refused() {
        let twice = read_a_broken_dialect(
            "a_dialect_naming_a_rule_twice",
            &ONE_DIALECT.replace("name = \"blank-line\"", "name = \"anything-at-all\""),
        );
        assert!(twice[0].contains("two rules both named anything-at-all"), "{twice:?}");

        let unnamed = read_a_broken_dialect(
            "a_dialect_with_a_nameless_rule",
            &ONE_DIALECT.replace("name = \"blank-line\"", "name = \"\""),
        );
        assert!(unnamed[0].contains("a rule with no name"), "{unnamed:?}");
    }

    #[test]
    fn the_buckets_are_exactly_the_ones_the_rules_count_into_and_both_directions_are_refused() {
        let unfillable = read_a_broken_dialect(
            "a_dialect_with_a_bucket_no_rule_fills",
            &ONE_DIALECT.replace("[\"code\", \"blanks\"]", "[\"code\", \"blanks\", \"comments\"]"),
        );
        assert!(unfillable[0].contains("no rule counts into comments"), "{unfillable:?}");

        let unlisted = read_a_broken_dialect(
            "a_dialect_counting_into_an_unlisted_bucket",
            &ONE_DIALECT.replace("bucket = \"blanks\"\n", "bucket = \"extra\"\n"),
        );
        assert!(unlisted.iter().any(|f| f.contains("rule blank-line: counts into extra")), "{unlisted:?}");
        assert!(unlisted.iter().any(|f| f.contains("no rule counts into blanks")), "{unlisted:?}");
    }

    #[test]
    fn a_dialect_with_no_rule_at_all_is_refused() {
        let refused = read_a_broken_dialect(
            "a_dialect_with_no_rule",
            "counter = \"tokei\"\ndialect = \"default\"\nbuckets = [\"code\"]\nrule = []\n",
        );
        assert!(refused[0].contains("holds no rule"), "{refused:?}");
    }

    #[test]
    fn a_directory_with_no_dialect_file_is_refused_instead_of_knowing_nothing() {
        let dir = env::temp_dir().join("linejudge-an_empty_dialects_dir");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let refused = Dialects::read(&dir)
            .err()
            .unwrap_or_else(|| panic!("an empty directory was read anyway"));
        assert!(refused[0].contains("holds no dialect file"), "{refused:?}");

        fs::write(dir.join("default.toml"), ONE_DIALECT).unwrap();
        let stray = Dialects::read(&dir)
            .err()
            .unwrap_or_else(|| panic!("a stray top-level file was read anyway"));
        fs::remove_dir_all(&dir).unwrap();
        assert!(stray[0].contains("counter's own folder"), "{stray:?}");
    }

    #[test]
    fn a_bucket_set_is_exactly_the_dialects_own_and_says_which_when_it_is_not() {
        let wanted: Vec<String> =
            ["code", "comments", "blanks"].iter().map(|n| n.to_string()).collect();
        let named = |names: [(&str, u32); 3]| {
            names.iter().map(|(n, v)| (n.to_string(), *v)).collect::<BTreeMap<_, _>>()
        };
        assert!(check_buckets(&named([("code", 1), ("comments", 1), ("blanks", 0)]), &wanted).is_ok());
        let missing = check_buckets(&named([("code", 1), ("comments", 1), ("extra", 0)]), &wanted);
        assert!(missing.unwrap_err().contains("no blanks bucket"));
    }

    fn read_a_broken_dialect(name: &str, text: &str) -> Vec<String> {
        let dir = env::temp_dir().join(format!("linejudge-{name}"));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("tokei").join("default.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        let read = Dialect::read(&path);
        fs::remove_dir_all(&dir).unwrap();
        match read {
            Ok(_) => panic!("the dialect was read without a fault"),
            Err(faults) => faults,
        }
    }
}
