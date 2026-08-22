//! The cases: one small input file each, with its strings and comments marked by hand.

use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::faults::Faults;
use crate::readings::{READINGS_FILE, Readings};
use crate::truth::Truth;

/// The directory the cases are read from, one directory per group inside it.
pub const CASES_DIR: &str = "cases";

pub(crate) const DISABLED_PREFIX: &str = "disabled-";

const CASE_FILE: &str = "case.toml";
const GROUP_SIZE: u32 = 1000;
const INPUT_STEM: &str = "input.";
const TRUTH_FILE: &str = "truth.txt";

/// Every case of the corpus, read from a directory holding one directory per group, each named
/// after a whole thousand, with the cases of that thousand inside it.
#[derive(Debug)]
pub struct Corpus {
    /// In the order their numbers put them.
    pub cases: Vec<Case>,
    /// The group directory names in numeric order. Which group a case belongs to is the thousand
    /// its number falls in.
    pub groups: Vec<String>,
    /// The cases set aside by the `disabled-` prefix on their directory, named without it. Their
    /// files are not read at all, since a case is usually disabled because something in it is
    /// broken and would otherwise fail the whole corpus.
    pub disabled: Vec<String>,
    /// The questions this corpus lets two counters answer differently.
    pub readings: Readings,
}

impl Corpus {
    /// The case a fragment of a name points at: the one named exactly that, or failing that the
    /// one case whose name holds it. Names are long, `1150-doc_comment_opening_after_code`, and
    /// nobody types them whole.
    ///
    /// `Err` carries every case the fragment fits, so it is empty where the fragment fits none and
    /// holds more than one where it is ambiguous. Nothing is ever guessed between them.
    pub fn find_case(&self, fragment: &str) -> Result<&Case, Vec<&Case>> {
        if let Some(exact) = self.cases.iter().find(|case| case.name == fragment) {
            return Ok(exact);
        }
        let fitting: Vec<&Case> =
            self.cases.iter().filter(|case| case.name.contains(fragment)).collect();
        match fitting.as_slice() {
            [one] => Ok(one),
            _ => Err(fitting),
        }
    }

    /// Reads every case under the directory. Failures are collected rather than stopped at, so a
    /// corpus with three broken cases reports all three.
    pub fn read(dir: &Path) -> Result<Corpus, Faults> {
        let readings = match Readings::read(dir) {
            Ok(readings) => readings,
            Err(message) => return Err(format!("{READINGS_FILE}: {message}").into()),
        };
        let groups = match find_the_directories_in(dir) {
            Ok(groups) => groups,
            Err(error) => {
                let where_it_is = dir.display();
                return Err(
                    format!("{where_it_is}: the corpus directory could not be opened: {error}")
                        .into(),
                );
            }
        };

        let mut cases = Vec::new();
        let mut named_groups = Vec::new();
        let mut disabled = Vec::new();
        let mut faults: Vec<String> = Vec::new();
        for group in groups {
            let group_name = get_name_of(&group);
            let first = match find_the_first_number_of(&group_name) {
                Ok(first) => first,
                Err(message) => {
                    faults.push(format!("{group_name}: {message}"));
                    continue;
                }
            };
            named_groups.push(group_name.clone());
            let inside = match find_the_directories_in(&group) {
                Ok(inside) => inside,
                Err(error) => {
                    faults.push(format!(
                        "{group_name}: the group directory could not be opened: {error}"
                    ));
                    continue;
                }
            };
            for path in inside {
                let name = get_name_of(&path);
                let disabled_name = find_disabled_name(&name);
                let named = disabled_name.clone().unwrap_or(name);
                if let Err(message) = check_the_number_of(&named, first, &group_name) {
                    faults.push(format!("{named}: {message}"));
                    continue;
                }
                match disabled_name {
                    Some(name) => disabled.push(name),
                    None => match Case::read(&path, &readings) {
                        Ok(case) => cases.push(case),
                        Err(found) => faults.extend(found.iter().cloned()),
                    },
                }
            }
        }
        disabled.sort();
        // A witness that a broken case fails to be would be blamed twice, so the check waits for
        // a corpus whose cases all read.
        if faults.is_empty() {
            check_witnesses(&readings, &cases, &mut faults);
        }
        match faults.is_empty() {
            true => Ok(Corpus { cases, groups: named_groups, disabled, readings }),
            false => Err(faults.into()),
        }
    }
}

/// One case: a small input file, the spans marked in it, and what it is trying to catch.
#[derive(Debug)]
pub struct Case {
    /// The directory name, number and words together, which is how the report and a
    /// known-failures file name it. The group is no part of it, so moving a case between groups
    /// is a renumbering and nothing else.
    pub name: String,
    /// The one file a counter is ever pointed at, `input.<extension>` inside the case directory.
    pub input_file: PathBuf,
    /// What this case is trying to catch, written for a person, and empty for a disabled case.
    pub trap: String,
    pub truth: Truth,
}

impl Case {
    /// Reads one case directory, given the readings its corpus defines.
    pub fn read(dir: &Path, readings: &Readings) -> Result<Case, Faults> {
        let name = get_name_of(dir);
        let one = |message: String| Faults::from(format!("{name}: {message}"));

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
                let named: Vec<String> = messages
                    .iter()
                    .map(|message| format!("{name}: {TRUTH_FILE}: {message}"))
                    .collect();
                return Err(named.into());
            }
        };
        for reading in truth.find_optional_readings() {
            if readings.find(reading).is_none() {
                faults.push(format!(
                    "{name}: {TRUTH_FILE} marks the reading {reading}, which {READINGS_FILE} does \
                     not define"
                ));
            }
        }
        if raw.trap.trim().is_empty() {
            faults.push(format!("{name}: the trap says nothing"));
        }
        if !faults.is_empty() {
            return Err(faults.into());
        }

        Ok(Case { name, input_file, trap: raw.trap, truth })
    }
}

fn find_disabled_name(name: &str) -> Option<String> {
    name.strip_prefix(DISABLED_PREFIX).map(|name| name.to_string())
}

fn get_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

// Sorted by the number a name begins with rather than by the name, so a case one digit longer
// than its neighbours still sits where its number says.
fn find_the_directories_in(dir: &Path) -> Result<Vec<PathBuf>, io::Error> {
    let mut found: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    found.sort_by_key(|path| {
        let name = get_name_of(path);
        (find_the_number_in(&name), name)
    });
    Ok(found)
}

fn find_the_first_number_of(group: &str) -> Result<u32, String> {
    match find_the_number_in(group) {
        Some(number) if number % GROUP_SIZE == 0 => Ok(number),
        _ => Err(format!(
            "a corpus holds groups and a group holds cases, so {group} has to be named after a \
             whole thousand, as <thousand>-<words>"
        )),
    }
}

// A case filed under the wrong group would keep working and stop being findable by number.
fn check_the_number_of(name_of_case: &str, first: u32, group: &str) -> Result<(), String> {
    match find_the_number_in(name_of_case) {
        Some(number) if (first..first.saturating_add(GROUP_SIZE)).contains(&number) => Ok(()),
        Some(_) => Err(format!(
            "it sits in {group}, whose cases are numbered {first} to {}",
            first.saturating_add(GROUP_SIZE - 1)
        )),
        None => Err(format!("{name_of_case} has to be named <number>-<words>")),
    }
}

fn find_the_number_in(name: &str) -> Option<u32> {
    name.split('-').next()?.parse().ok()
}

// A case holds exactly one `input.<extension>`, and its extension is what tells a counter which
// language to read it as.
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

fn check_witnesses(readings: &Readings, cases: &[Case], faults: &mut Vec<String>) {
    for (name, reading) in readings.iter() {
        let one = |message: String| format!("{READINGS_FILE}: {message}");
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

// A case file holds the trap and nothing else. What a counter printed lives under `recorded/`, so
// an answer block written here is refused rather than ignored.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCase {
    trap: String,
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    const A_GROUP: &str = "0000-a_group_built_by_a_test";
    const ONE_CASE: &str = "trap = \"\"\"\n\
                            a line comment inside a block comment is part of the block\"\"\"\n";

    #[test]
    fn every_case_of_the_corpus_is_read_without_a_fault() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("cases");
        match Corpus::read(&dir) {
            Ok(corpus) => {
                assert!(!corpus.cases.is_empty(), "{} holds no case", dir.display());
                assert_eq!(corpus.groups.len(), 8, "{:?}", corpus.groups);
                assert_eq!(corpus.groups[0], "1000-comments");
                assert_eq!(corpus.groups[7], "8000-what_the_line_counts_as");
            }
            Err(faults) => {
                let report: Vec<String> = faults.iter().map(|f| f.to_string()).collect();
                panic!("{}", report.join("\n"));
            }
        }
    }

    #[test]
    fn a_fragment_names_a_case_where_it_fits_exactly_one_and_never_guesses() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("cases");
        let corpus = Corpus::read(&dir).unwrap_or_else(|faults| panic!("{faults:?}"));

        let whole = "1150-doc_comment_opening_after_code";
        assert_eq!(corpus.find_case(whole).unwrap_or_else(|_| panic!("{whole}")).name, whole);
        assert_eq!(corpus.find_case("1150").unwrap_or_else(|_| panic!("1150")).name, whole);
        assert!(corpus.find_case("no_case_is_called_this").unwrap_err().is_empty());

        let many = corpus.find_case("doc_comment").unwrap_err();
        assert!(many.len() > 1, "{:?}", many.iter().map(|c| &c.name).collect::<Vec<_>>());
    }

    #[test]
    fn a_case_that_carries_no_truth_is_refused() {
        let root = env::temp_dir().join("linejudge-a_case_with_no_truth");
        let dir = root.join(A_GROUP).join("0400-a_case_built_by_a_test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("input.c"), "/* a block\n*/ int x = 1;\n").unwrap();
        fs::write(dir.join(CASE_FILE), ONE_CASE).unwrap();
        let faults = Corpus::read(&root)
            .err()
            .unwrap_or_else(|| panic!("it was read anyway"));
        fs::remove_dir_all(&root).unwrap();
        assert!(faults[0].contains("is not there"), "{faults:?}");
    }

    #[test]
    fn a_case_that_still_carries_an_answer_block_is_refused() {
        let case = ONE_CASE.to_string()
            + "\n[answer.tokei.default]\n\
               counted = { lines = 2, code = 1, comments = 1, blanks = 0 }\n";
        let faults = read_a_broken_case("a_case_still_holding_answers", &case);
        assert!(faults[0].contains("unknown field `answer`"), "{faults:?}");
    }

    #[test]
    fn a_disabled_case_is_set_aside_by_its_directory_name_and_never_read() {
        let root = env::temp_dir().join("linejudge-a_disabled_case");
        let kept = root.join(A_GROUP).join("0400-a_case_built_by_a_test");
        let aside = root.join(A_GROUP).join("disabled-0500-a_case_nobody_trusts");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&kept).unwrap();
        fs::create_dir_all(&aside).unwrap();
        fs::write(kept.join("input.c"), "/* a block\n*/ int x = 1;\n").unwrap();
        fs::write(kept.join(TRUTH_FILE), "/* a block\nCCcccccccc\n*/ int x = 1;\nUU ... . . ..\n")
            .unwrap();
        fs::write(kept.join(CASE_FILE), ONE_CASE).unwrap();
        fs::write(aside.join("input.c"), "not even a file a counter could read").unwrap();
        let corpus = Corpus::read(&root)
            .unwrap_or_else(|faults| panic!("{faults:?}"));
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(corpus.cases.len(), 1);
        assert_eq!(corpus.disabled, ["0500-a_case_nobody_trusts"]);
    }

    #[test]
    fn a_case_outside_its_groups_thousand_is_refused_and_so_is_a_group_that_is_not_one() {
        let root = env::temp_dir().join("linejudge-a_case_in_the_wrong_group");
        let stray = root.join(A_GROUP).join("1400-a_case_of_another_thousand");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&stray).unwrap();
        fs::write(stray.join("input.c"), "/* a block\n*/ int x = 1;\n").unwrap();
        let misfiled = Corpus::read(&root)
            .err()
            .unwrap_or_else(|| panic!("it was read anyway"));

        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("1234-not_a_whole_thousand").join("1240-a_case")).unwrap();
        let ungrouped = Corpus::read(&root)
            .err()
            .unwrap_or_else(|| panic!("it was read anyway"));
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(misfiled.len(), 1, "{misfiled:?}");
        assert!(misfiled[0].starts_with("1400-a_case_of_another_thousand:"), "{misfiled:?}");
        assert!(misfiled[0].contains("numbered 0 to 999"), "{misfiled:?}");
        assert!(ungrouped[0].contains("whole thousand"), "{ungrouped:?}");
    }

    #[test]
    fn a_number_one_digit_longer_sorts_by_what_it_is_and_not_by_how_it_reads() {
        let root = env::temp_dir().join("linejudge-a_corpus_of_two_widths");
        let _ = fs::remove_dir_all(&root);
        for (group, case) in [("2000-second", "2010-second"), ("10000-tenth", "10010-tenth")] {
            let dir = root.join(group).join(case);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("input.c"), "/* a block\n*/ int x = 1;\n").unwrap();
            fs::write(dir.join(TRUTH_FILE), "/* a block\nCCcccccccc\n*/ int x = 1;\nUU ... . . ..\n")
                .unwrap();
            fs::write(dir.join(CASE_FILE), ONE_CASE).unwrap();
        }
        let corpus = Corpus::read(&root).unwrap_or_else(|faults| panic!("{faults:?}"));
        fs::remove_dir_all(&root).unwrap();
        let named: Vec<&str> = corpus.cases.iter().map(|case| case.name.as_str()).collect();
        assert_eq!(named, ["2010-second", "10010-tenth"]);
    }

    #[test]
    fn a_truth_marking_a_reading_the_corpus_does_not_define_is_refused() {
        let faults = build_and_read_the_case(
            "a_reading_nobody_defined",
            ONE_CASE,
            "input.rs",
            "/** doc */\nlet x = 1\n",
            "/** doc */\nCCCcccccUU Markdown (optional js-jsdoc)\nlet x = 1\n... . . .\n",
        )
        .err()
        .unwrap_or_else(|| panic!("the case was read anyway"));
        let wanted = "marks the reading js-jsdoc, which readings.toml does not define";
        assert!(faults.iter().any(|fault| fault.contains(wanted)), "{faults:?}");
    }

    #[test]
    fn the_witness_of_a_reading_has_to_exist_and_to_mark_it() {
        let root = env::temp_dir().join("linejudge-a_witness_that_is_not_there");
        let dir = root.join(A_GROUP).join("0400-a_case_built_by_a_test");
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
            Corpus::read(&root)
                .err()
                .unwrap_or_else(|| panic!("it was read anyway"))
        };

        fs::write(root.join("readings.toml"), reading("0500-not_here")).unwrap();
        let missing = read_and_refuse();
        assert!(missing[0].contains("there is no case of that name"), "{missing:?}");

        fs::write(root.join("readings.toml"), reading("0400-a_case_built_by_a_test")).unwrap();
        let unmarked = read_and_refuse();
        fs::remove_dir_all(&root).unwrap();
        assert!(unmarked[0].contains("marks no such reading"), "{unmarked:?}");
    }

    // A trap saying "case 1070 is the same line with a space" is the only place one case points at
    // another, and renumbering 1070 would leave that sentence pointing nowhere.
    #[test]
    fn every_case_a_trap_points_at_is_in_the_corpus() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("cases");
        let corpus = Corpus::read(&dir).unwrap_or_else(|faults| panic!("{faults:?}"));
        let mut checked = 0;
        let mut written = 0;
        for case in &corpus.cases {
            for number in find_the_case_numbers_in(&case.trap) {
                let exists =
                    corpus.cases.iter().any(|one| find_the_number_in(&one.name) == Some(number));
                assert!(exists, "{}: its trap points at case {number}, which is gone", case.name);
                checked += 1;
            }
            // Counted again the plain way, so a walk that quietly finds fewer than are there
            // cannot leave this test passing while it guards a part of them.
            let lowered = case.trap.to_lowercase();
            for (at, _) in lowered.match_indices("case ") {
                let after = &lowered[at + "case ".len()..];
                written += usize::from(after.starts_with(|one: char| one.is_ascii_digit()));
            }
        }
        assert!(written > 0, "no trap points at another case, so this test proved nothing");
        assert_eq!(checked, written, "the walk over the traps missed a reference");
    }

    fn find_the_case_numbers_in(trap: &str) -> Vec<u32> {
        const NAMES_ONE: &str = "case ";

        let lowered = trap.to_lowercase();
        let mut rest = lowered.as_str();
        let mut found = Vec::new();
        while let Some(at) = rest.find(NAMES_ONE) {
            rest = &rest[at + NAMES_ONE.len()..];
            let digits: String = rest.chars().take_while(|one| one.is_ascii_digit()).collect();
            rest = &rest[digits.len()..];
            let ends_the_word = rest.chars().next().is_none_or(|one| !one.is_alphanumeric());
            if ends_the_word && let Ok(number) = digits.parse() {
                found.push(number);
            }
        }
        found
    }

    fn build_and_read_the_case(
        name: &str,
        declaration: &str,
        input_file: &str,
        input: &str,
        marked: &str,
    ) -> Result<Corpus, Faults> {
        let root = env::temp_dir().join(format!("linejudge-{name}"));
        let dir = root.join(A_GROUP).join("0400-a_case_built_by_a_test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(input_file), input).unwrap();
        fs::write(dir.join(TRUTH_FILE), marked).unwrap();
        fs::write(dir.join(CASE_FILE), declaration).unwrap();
        let read = Corpus::read(&root);
        fs::remove_dir_all(&root).unwrap();
        read
    }

    fn read_a_broken_case(name: &str, declaration: &str) -> Faults {
        let read = build_and_read_the_case(
            name,
            declaration,
            "input.c",
            "/* a block\n*/ int x = 1;\n",
            "/* a block\nCCcccccccc\n*/ int x = 1;\nUU ... . . ..\n",
        );
        match read {
            Ok(_) => panic!("the case was read without a fault"),
            Err(faults) => faults,
        }
    }
}
