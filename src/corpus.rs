use std::fmt;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::readings::{READINGS_FILE, Readings};
use crate::truth::Truth;

pub const DISABLED_PREFIX: &str = "disabled-";

const CASE_FILE: &str = "case.toml";
const GROUP_SIZE: u32 = 1000;
const INPUT_STEM: &str = "input.";
const TRUTH_FILE: &str = "truth.txt";

/// Every case of the corpus, read from a directory holding one directory per group, each named
/// after a whole thousand, with the cases of that thousand inside it.
pub struct Corpus {
    pub cases: Vec<Case>,
    /// The cases set aside by the `disabled-` prefix on their directory, named without it. Their
    /// files are not read at all, since a case is usually disabled because something in it is
    /// broken, and a broken truth would otherwise fail the whole corpus.
    pub disabled: Vec<String>,
    pub readings: Readings,
}

impl Corpus {
    pub fn read(dir: &Path) -> Result<Corpus, Vec<Fault>> {
        let readings = match Readings::read(dir) {
            Ok(readings) => readings,
            Err(message) => {
                return Err(vec![Fault { case: READINGS_FILE.to_string(), message }]);
            }
        };
        let groups = match find_the_directories_in(dir) {
            Ok(groups) => groups,
            Err(error) => {
                return Err(vec![Fault {
                    case: dir.display().to_string(),
                    message: format!("the corpus directory could not be opened: {error}"),
                }]);
            }
        };

        let mut cases = Vec::new();
        let mut disabled = Vec::new();
        let mut faults = Vec::new();
        for group in groups {
            let group_name = get_name_of(&group);
            let first = match find_the_first_number_of(&group_name) {
                Ok(first) => first,
                Err(message) => {
                    faults.push(Fault { case: group_name, message });
                    continue;
                }
            };
            let inside = match find_the_directories_in(&group) {
                Ok(inside) => inside,
                Err(error) => {
                    let message = format!("the group directory could not be opened: {error}");
                    faults.push(Fault { case: group_name, message });
                    continue;
                }
            };
            for path in inside {
                let name = get_name_of(&path);
                let disabled_name = find_disabled_name(&name);
                let named = disabled_name.clone().unwrap_or(name);
                if let Err(message) = check_the_number_of(&named, first, &group_name) {
                    faults.push(Fault { case: named, message });
                    continue;
                }
                match disabled_name {
                    Some(name) => disabled.push(name),
                    None => match Case::read(&path, &readings) {
                        Ok(case) => cases.push(case),
                        Err(mut found) => faults.append(&mut found),
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
        if faults.is_empty() {
            Ok(Corpus { cases, disabled, readings })
        } else {
            Err(faults)
        }
    }
}

/// `name` is the case's own directory name, number and words together, and it is how a case is
/// named in the report and in a known-failures file. The group it sits in is no part of it, so
/// moving a case between groups is a renumbering and nothing else.
pub struct Case {
    pub name: String,
    pub input_file: PathBuf,
    pub trap: String,
    pub truth: Truth,
}

impl Case {
    pub fn read(dir: &Path, readings: &Readings) -> Result<Case, Vec<Fault>> {
        let name = get_name_of(dir);
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
        if raw.trap.trim().is_empty() {
            faults.push(Fault { case: name.clone(), message: "the trap says nothing".to_string() });
        }
        if !faults.is_empty() {
            return Err(faults);
        }

        Ok(Case { name, input_file, trap: raw.trap, truth })
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

fn find_disabled_name(name: &str) -> Option<String> {
    name.strip_prefix(DISABLED_PREFIX).map(|name| name.to_string())
}

fn get_name_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Sorted by the number each name begins with and not by the name, so that a group or a case whose
/// number is one digit longer than its neighbours still sits where the number says.
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

/// A case filed under the wrong group would keep working and stop being findable by anybody
/// reading the corpus by number, so the group a case sits in has to agree with the case's own
/// number.
fn check_the_number_of(case: &str, first: u32, group: &str) -> Result<(), String> {
    match find_the_number_in(case) {
        Some(number) if (first..first.saturating_add(GROUP_SIZE)).contains(&number) => Ok(()),
        Some(_) => Err(format!(
            "it sits in {group}, whose cases are numbered {first} to {}",
            first.saturating_add(GROUP_SIZE - 1)
        )),
        None => Err(format!("{case} has to be named <number>-<words>")),
    }
}

fn find_the_number_in(name: &str) -> Option<u32> {
    name.split('-').next()?.parse().ok()
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

/// A case file holds the trap and nothing else. What each counter once printed lives in that
/// counter's own file under `recorded/`, so a case left holding an answer block is a case the
/// move missed, and the unknown field refuses it loudly.
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
        let dir = root.join(A_GROUP).join("0400-a_case_built_by_a_test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("input.c"), "/* a block\n*/ int x = 1;\n").unwrap();
        fs::write(dir.join(CASE_FILE), ONE_CASE).unwrap();
        let faults = Corpus::read(&root)
            .err()
            .unwrap_or_else(|| panic!("it was read anyway"));
        fs::remove_dir_all(&root).unwrap();
        assert!(faults[0].message.contains("is not there"), "{faults:?}");
    }

    #[test]
    fn a_case_that_still_carries_an_answer_block_is_refused() {
        let case = ONE_CASE.to_string()
            + "\n[answer.tokei.default]\n\
               counted = { lines = 2, code = 1, comments = 1, blanks = 0 }\n";
        let faults = read_a_broken_case("a_case_still_holding_answers", &case);
        assert!(faults[0].message.contains("unknown field `answer`"), "{faults:?}");
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
        assert_eq!(misfiled[0].case, "1400-a_case_of_another_thousand");
        assert!(misfiled[0].message.contains("numbered 0 to 999"), "{misfiled:?}");
        assert!(ungrouped[0].message.contains("whole thousand"), "{ungrouped:?}");
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
        assert!(faults.iter().any(|f| f.message.contains(wanted)), "{faults:?}");
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
        assert!(missing[0].message.contains("there is no case of that name"), "{missing:?}");

        fs::write(root.join("readings.toml"), reading("0400-a_case_built_by_a_test")).unwrap();
        let unmarked = read_and_refuse();
        fs::remove_dir_all(&root).unwrap();
        assert!(unmarked[0].message.contains("marks no such reading"), "{unmarked:?}");
    }

    fn build_and_read_the_case(
        name: &str,
        declaration: &str,
        input_file: &str,
        input: &str,
        marked: &str,
    ) -> Result<Corpus, Vec<Fault>> {
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

    fn read_a_broken_case(name: &str, declaration: &str) -> Vec<Fault> {
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
