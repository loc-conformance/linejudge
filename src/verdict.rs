use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use crate::adapter::{Adapter, Dialect};
use crate::answer::Answer;
use crate::corpus::{Case, Corpus};
use crate::deriver::derive_answer;
use crate::dialects;
use crate::recorded::{Exception, RecordedAnswer, RecordedAnswers, is_same_build};

const AT_ONCE: usize = 8;

/// Whether a counter does what its own rules say. This is the whole of what can be asked of a
/// counter nobody here has photographed, and it never needs a recorded answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conformance {
    Agrees,
    Fails,
    /// The counter says there is no such file, which is an answer of its own and not a failure.
    Unclaimed,
}

/// Whether a counter still does what it did when it was photographed. Only asked where a recorded
/// answer exists and the running build is the recorded one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drift {
    Same,
    Changed,
    NoLongerClaimed,
    NowClaimed,
}

/// One counter's answer to one case. A counter that breaks on a case has answered it: the trouble
/// is this case's outcome, carrying the message, and every other case is measured anyway.
pub struct Judged<'a> {
    pub case: &'a Case,
    pub outcome: Outcome<'a>,
}

impl Judged<'_> {
    pub fn breaks_the_run(&self) -> bool {
        match &self.outcome {
            Outcome::Broke(_) => true,
            Outcome::Measured(measured) => measured.breaks_the_run(),
        }
    }
}

pub enum Outcome<'a> {
    /// A non-zero exit, a panic, or output that could not be read, with what is known about why.
    Broke(String),
    Measured(Measured<'a>),
}

pub struct Measured<'a> {
    /// What the counter ought to answer by its own rules, or by its exception where one is
    /// declared.
    pub real: Answer,
    /// What it answered now. `None` is a counter that claims no such file.
    pub live: Option<Answer>,
    pub record: Option<&'a RecordedAnswer>,
    pub exception: Option<&'a Exception>,
    pub conformance: Conformance,
    /// `None` where there is no photograph to hold the run against, or the build differs from the
    /// recorded one.
    pub drift: Option<Drift>,
}

impl Measured<'_> {
    /// A failure that fails exactly as the photograph says is known, and known is the one failure
    /// that does not break the run.
    pub fn is_a_known_failure(&self) -> bool {
        self.conformance == Conformance::Fails && self.drift == Some(Drift::Same)
    }

    /// What a known-failures list is asked to name: a wrong answer, or a change in what the
    /// counter claims at all.
    pub fn is_a_failure(&self) -> bool {
        self.conformance == Conformance::Fails
            || matches!(self.drift, Some(Drift::NoLongerClaimed | Drift::NowClaimed))
    }

    pub fn breaks_the_run(&self) -> bool {
        self.is_a_failure() && !self.is_a_known_failure()
    }

    pub fn agrees_through_its_exception(&self) -> bool {
        self.exception.is_some() && self.conformance == Conformance::Agrees
    }
}

/// Runs the counter once per case, so this is as slow as the counter is, times the corpus.
/// `Err` is never the counter's doing: it is this suite's own data refusing to judge, a case no
/// rule can place or a recorded flag its own numbers contradict.
pub fn measure_and_judge_every_case<'a>(
    adapter: &Adapter,
    dialect: &Dialect,
    rules: &dialects::Dialect,
    binary: &Path,
    corpus: &'a Corpus,
    record: Option<&'a RecordedAnswers>,
    version_of_this_run: &str,
) -> Result<Vec<Judged<'a>>, Vec<String>> {
    let drift_is_judged =
        record.is_some_and(|record| is_same_build(&record.version, version_of_this_run));
    let key = format!("{}.{}", adapter.name_of_counter, dialect.name);

    let mut prepared = Vec::with_capacity(corpus.cases.len());
    let mut faults = Vec::new();
    for case in &corpus.cases {
        let exception = record.and_then(|r| r.find_exception(&case.name, &dialect.name));
        let real = match exception {
            Some(exception) => exception.expected.clone(),
            None => match derive_answer(&case.truth, rules, &corpus.readings) {
                Ok(derivation) => derivation.real,
                Err(messages) => {
                    faults.extend(messages.into_iter().map(|m| format!("{}: {m}", case.name)));
                    continue;
                }
            },
        };
        let entry = record.and_then(|r| r.find(&case.name, &dialect.name));
        if let Some(entry) = entry
            && let Some(counted) = &entry.counted
            && entry.is_known_failure == (*counted == real)
        {
            faults.push(match entry.is_known_failure {
                true => format!(
                    "{}: the record calls {key} a known failure, and its numbers agree with the \
                     rules, so the flag is stale",
                    case.name
                ),
                false => format!(
                    "{}: the record's numbers for {key} differ from what the rules ask, and the \
                     block does not say is-known-failure",
                    case.name
                ),
            });
        }
        prepared.push((case, real, entry, exception));
    }
    if !faults.is_empty() {
        return Err(faults);
    }

    let files: Vec<&Path> = prepared.iter().map(|(case, ..)| case.input_file.as_path()).collect();
    let answers = measure_every_file(adapter, dialect, binary, &files);

    let mut judged = Vec::with_capacity(prepared.len());
    for ((case, real, entry, exception), answer) in prepared.into_iter().zip(answers) {
        let live = match answer {
            Ok(live) => live,
            Err(message) => {
                judged.push(Judged { case, outcome: Outcome::Broke(message) });
                continue;
            }
        };
        let conformance = judge_conformance(&real, live.as_ref());
        let drift = match (drift_is_judged, entry) {
            (true, Some(entry)) => Some(judge_drift(entry, live.as_ref())),
            _ => None,
        };
        judged.push(Judged {
            case,
            outcome: Outcome::Measured(Measured {
                real,
                live,
                record: entry,
                exception,
                conformance,
                drift,
            }),
        });
    }
    Ok(judged)
}

/// Runs the counter over every file at once, a few at a time, and hands the answers back in the
/// order the files were given. Nearly all of the time is spent waiting for somebody else's program
/// to start and finish, so the number running together is well above the number of cores.
fn measure_every_file(
    adapter: &Adapter,
    dialect: &Dialect,
    binary: &Path,
    files: &[&Path],
) -> Vec<Result<Option<Answer>, String>> {
    let next = AtomicUsize::new(0);
    let answers = Mutex::new(Vec::with_capacity(files.len()));
    let running = AT_ONCE.min(files.len().max(1));
    thread::scope(|scope| {
        for _ in 0..running {
            scope.spawn(|| {
                loop {
                    let at = next.fetch_add(1, Ordering::Relaxed);
                    let Some(file) = files.get(at) else { return };
                    let answered = adapter.measure(dialect, binary, file);
                    answers.lock().unwrap_or_else(|held| held.into_inner()).push((at, answered));
                }
            });
        }
    });
    let mut answers = answers.into_inner().unwrap_or_else(|held| held.into_inner());
    answers.sort_by_key(|(at, _)| *at);
    answers.into_iter().map(|(_, answered)| answered).collect()
}

pub fn judge_conformance(real: &Answer, live: Option<&Answer>) -> Conformance {
    match live {
        None => Conformance::Unclaimed,
        Some(live) if live == real => Conformance::Agrees,
        Some(_) => Conformance::Fails,
    }
}

pub fn judge_drift(record: &RecordedAnswer, live: Option<&Answer>) -> Drift {
    match (&record.counted, live) {
        (None, None) => Drift::Same,
        (None, Some(_)) => Drift::NowClaimed,
        (Some(_), None) => Drift::NoLongerClaimed,
        (Some(counted), Some(live)) if counted == live => Drift::Same,
        _ => Drift::Changed,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::env;
    use std::fs;

    use super::*;
    use crate::adapter::Reader;
    use crate::answer::Counts;
    use crate::dialects::{Dialects, read_the_shipped_dialects};
    use crate::measurement::OutputFormat;
    use crate::recorded::RecordedAnswers;

    #[test]
    fn a_counter_that_answers_what_its_rules_ask_agrees_and_one_that_does_not_fails() {
        assert_eq!(judge_conformance(&a_measurement(2), Some(&a_measurement(2))), Conformance::Agrees);
        assert_eq!(judge_conformance(&a_measurement(2), Some(&a_measurement(3))), Conformance::Fails);
        assert_eq!(judge_conformance(&a_measurement(2), None), Conformance::Unclaimed);
    }

    #[test]
    fn drift_holds_the_run_against_the_photograph_and_claims_are_answers_too() {
        let failure = a_record(Some(3), true);
        assert_eq!(judge_drift(&failure, Some(&a_measurement(3))), Drift::Same);
        assert_eq!(judge_drift(&failure, Some(&a_measurement(2))), Drift::Changed);
        assert_eq!(judge_drift(&failure, None), Drift::NoLongerClaimed);
        let unclaimed = a_record(None, false);
        assert_eq!(judge_drift(&unclaimed, None), Drift::Same);
        assert_eq!(judge_drift(&unclaimed, Some(&a_measurement(2))), Drift::NowClaimed);
    }

    #[test]
    fn the_known_failure_is_the_one_failure_that_breaks_nothing() {
        let known = measured(Conformance::Fails, Some(Drift::Same));
        assert!(known.is_a_known_failure());
        assert!(known.is_a_failure());
        assert!(!known.breaks_the_run());

        let new = measured(Conformance::Fails, Some(Drift::Changed));
        assert!(!new.is_a_known_failure());
        assert!(new.breaks_the_run());

        let unrecorded = measured(Conformance::Fails, None);
        assert!(unrecorded.breaks_the_run());

        let fixed = measured(Conformance::Agrees, Some(Drift::Changed));
        assert!(!fixed.is_a_failure());
        assert!(!fixed.breaks_the_run());
    }

    #[test]
    fn a_change_in_what_the_counter_claims_breaks_the_run() {
        let gone = measured(Conformance::Unclaimed, Some(Drift::NoLongerClaimed));
        assert!(gone.is_a_failure());
        assert!(gone.breaks_the_run());

        let appeared = measured(Conformance::Agrees, Some(Drift::NowClaimed));
        assert!(appeared.is_a_failure());
        assert!(appeared.breaks_the_run());

        let quiet = measured(Conformance::Unclaimed, None);
        assert!(!quiet.is_a_failure());
    }

    // The contradiction is found while the answers are prepared, before any binary runs, which is
    // why a binary that does not exist can stand in for the counter here.
    #[test]
    fn a_stale_failure_flag_is_refused_before_anything_is_run() {
        let dialects = read_the_shipped_dialects();
        let root = env::temp_dir().join("linejudge-a_stale_flag");
        let record_text = "counter = \"tokei\"\nversion = \"tokei 14.0.0\"\n\n\
                           [answer.0400-a_case_built_by_a_test.default]\n\
                           is-known-failure = true\n\
                           counted = { lines = 2, code = 1, comments = 1, blanks = 0 }\n";
        let faults = judge_a_built_corpus(&root, record_text, &dialects)
            .err()
            .unwrap_or_else(|| panic!("the stale flag was read anyway"));
        assert!(faults[0].contains("the flag is stale"), "{faults:?}");
    }

    #[test]
    fn a_recorded_failure_that_does_not_say_so_is_refused() {
        let dialects = read_the_shipped_dialects();
        let root = env::temp_dir().join("linejudge-an_unflagged_failure");
        let record_text = "counter = \"tokei\"\nversion = \"tokei 14.0.0\"\n\n\
                           [answer.0400-a_case_built_by_a_test.default]\n\
                           counted = { lines = 2, code = 2, comments = 0, blanks = 0 }\n";
        let faults = judge_a_built_corpus(&root, record_text, &dialects)
            .err()
            .unwrap_or_else(|| panic!("the unflagged failure was read anyway"));
        assert!(faults[0].contains("does not say is-known-failure"), "{faults:?}");
    }

    // The two lines derive as one comment and one code line, so a record saying the same numbers
    // with no flag is consistent, and nothing here needs the counter to exist until it is run.
    fn judge_a_built_corpus(
        root: &std::path::Path,
        record_text: &str,
        dialects: &Dialects,
    ) -> Result<Vec<Judged<'static>>, Vec<String>> {
        let cases = root.join("cases");
        let dir = cases.join("0000-a_group_built_by_a_test").join("0400-a_case_built_by_a_test");
        let _ = fs::remove_dir_all(root);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("input.c"), "/* a block\n*/ int x = 1;\n").unwrap();
        fs::write(dir.join("truth.txt"), "/* a block\nCCcccccccc\n*/ int x = 1;\nUU ... . . ..\n")
            .unwrap();
        fs::write(dir.join("case.toml"), "trap = \"\"\"\na block\"\"\"\n").unwrap();
        let recorded_dir = root.join("recorded");
        fs::create_dir_all(&recorded_dir).unwrap();
        fs::write(recorded_dir.join("tokei.toml"), record_text).unwrap();

        let corpus = Box::leak(Box::new(
            Corpus::read(&cases).unwrap_or_else(|faults| panic!("{faults:?}")),
        ));
        let record = Box::leak(Box::new(
            RecordedAnswers::read(std::slice::from_ref(&recorded_dir), "tokei", dialects)
                .unwrap_or_else(|faults| panic!("{faults:?}"))
                .unwrap_or_else(|| panic!("no record was read")),
        ));
        let adapter = a_tokei_adapter();
        let rules = dialects.find("tokei", "default").unwrap_or_else(|| panic!("no tokei rules"));
        let judged = measure_and_judge_every_case(
            &adapter,
            &adapter.dialects[0],
            rules,
            Path::new("a-binary-that-is-never-run"),
            corpus,
            Some(record),
            "tokei 14.0.0",
        );
        let _ = fs::remove_dir_all(root);
        judged
    }

    fn a_tokei_adapter() -> Adapter {
        Adapter {
            name_of_counter: "tokei".to_string(),
            args: vec!["{file}".to_string()],
            explain_args: None,
            explain_output: None,
            explain_keep_from: None,
            version_flag: None,
            acquisition: None,
            dialects: vec![Dialect {
                name: "default".to_string(),
                args: Vec::new(),
                buckets: vec!["code".to_string(), "comments".to_string(), "blanks".to_string()],
                reader: Reader::Written(OutputFormat::TokeiJson),
            }],
        }
    }

    fn measured(conformance: Conformance, drift: Option<Drift>) -> Measured<'static> {
        Measured {
            real: a_measurement(2),
            live: None,
            record: None,
            exception: None,
            conformance,
            drift,
        }
    }

    fn a_record(counted_code: Option<u32>, is_known_failure: bool) -> RecordedAnswer {
        RecordedAnswer {
            counted: counted_code.map(a_measurement),
            is_known_failure,
            note: None,
        }
    }

    fn a_measurement(code: u32) -> Answer {
        Answer {
            counts: Counts {
                lines: 4,
                buckets: BTreeMap::from([
                    ("code".to_string(), code),
                    ("comments".to_string(), 4 - code),
                    ("blanks".to_string(), 0),
                ]),
            },
            regions: Vec::new(),
        }
    }
}
