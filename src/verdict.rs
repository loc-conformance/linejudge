//! Runs a counter over the corpus and says, per case, whether it did what its rules say.

use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use crate::adapter::{Adapter, Invocation};
use crate::answer::Answer;
use crate::corpus::{Case, Corpus};
use crate::deriver::derive_answer;
use crate::dialects::Dialects;
use crate::faults::Faults;
use crate::known_failures::KnownFailures;
use crate::recorded::{Exception, RecordedAnswer, RecordedAnswers, is_same_build};

const AT_ONCE: usize = 8;

/// Whether a counter does what its own rules say. This is the whole of what can be asked of a
/// counter nothing was ever recorded about, and it needs no record at all.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Conformance {
    /// It answered exactly what its rules ask for.
    Agrees,
    /// It answered something else.
    Fails,
    /// The counter says there is no such file, which is an answer of its own and not a failure.
    Unclaimed,
}

/// Whether a counter still answers the way it did when it was last recorded. Only asked where a
/// record exists and the running build is the recorded one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drift {
    /// The same answer as the record holds.
    Same,
    /// Different numbers.
    Changed,
    /// It used to claim the file and now says there is no such file.
    NoLongerClaimed,
    /// It used to say there is no such file and now claims it.
    NowClaimed,
}

/// One counter's answer to one case.
#[derive(Debug)]
pub struct Judged<'a> {
    /// The case it was run over.
    pub case: &'a Case,
    /// What came of running it.
    pub outcome: Outcome<'a>,
}

impl Judged<'_> {
    fn breaks_a_run_with_no_list(&self) -> bool {
        match &self.outcome {
            Outcome::Broke(_) => true,
            Outcome::Measured(measured) => {
                measured.is_a_failure() && !measured.fails_exactly_as_recorded()
            }
        }
    }
}

/// What came of asking one counter about one case. Breaking is an outcome like any other, and the
/// rest of the corpus is measured anyway.
#[derive(Debug)]
pub enum Outcome<'a> {
    /// A non-zero exit, a panic, or output that could not be read, with what is known about why.
    Broke(String),
    /// It answered, and the answer was judged.
    Measured(Measured<'a>),
}

/// One judged answer.
#[derive(Debug)]
pub struct Measured<'a> {
    /// What it ought to answer by its own rules, or by its exception where one is declared.
    pub real: Answer,
    /// What it answered now. `None` is a counter that claims no such file.
    pub live: Option<Answer>,
    /// What it answered when it was last recorded, and `None` where there is no record.
    pub record: Option<&'a RecordedAnswer>,
    /// The exception this case was judged through, and `None` for the ordinary case.
    pub exception: Option<&'a Exception>,
    /// Whether it did what its own rules say, which is the verdict this suite exists to give.
    pub conformance: Conformance,
    /// Whether it still answers the way it did. `None` where there is no record to hold the run
    /// against, or the build being run is not the recorded one.
    pub drift: Option<Drift>,
}

impl Measured<'_> {
    /// Whether it fails in exactly the way the record holds. This asks the record and never a
    /// known-failures list, which is a different question and belongs to whoever wrote the list:
    /// see [`find_what_breaks_the_run`].
    pub fn fails_exactly_as_recorded(&self) -> bool {
        self.conformance == Conformance::Fails && self.drift == Some(Drift::Same)
    }

    /// What a known-failures list is asked to name: a wrong answer, or a change in what the
    /// counter claims at all.
    pub fn is_a_failure(&self) -> bool {
        self.conformance == Conformance::Fails
            || matches!(self.drift, Some(Drift::NoLongerClaimed | Drift::NowClaimed))
    }

    /// Whether it agrees only because an exception was declared for this case.
    pub fn agrees_through_its_exception(&self) -> bool {
        self.exception.is_some() && self.conformance == Conformance::Agrees
    }
}

/// The cases that should fail the build of whoever ran this, which is nothing where they all pass.
///
/// `allowed` is the counter's own list, the file in its own repository saying which cases it is
/// content to fail. With one, a case counts here when it failed or broke and the list does not
/// name it. Without one, the record stands in: a case counts when it fails in a way the record
/// does not already hold.
pub fn find_what_breaks_the_run<'a, 'c>(
    judged: &'a [Judged<'c>],
    name_of_dialect: &str,
    allowed: Option<&KnownFailures>,
) -> Vec<&'a Judged<'c>> {
    judged
        .iter()
        .filter(|one| match allowed {
            None => one.breaks_a_run_with_no_list(),
            Some(list) => match &one.outcome {
                Outcome::Broke(_) => !list.names(name_of_dialect, &one.case.name),
                Outcome::Measured(measured) => {
                    measured.is_a_failure() && !list.names(name_of_dialect, &one.case.name)
                }
            },
        })
        .collect()
}

/// Runs the counter once per case and judges each answer. `Err` is never the counter's doing: it
/// is this suite's own data refusing to judge, a case no rule can place or a recorded flag its own
/// numbers contradict.
pub fn measure_and_judge_every_case<'a>(
    adapter: &Adapter,
    invocation: &Invocation,
    dialects: &Dialects,
    binary: &Path,
    corpus: &'a Corpus,
    record: Option<&'a RecordedAnswers>,
    version_of_this_run: &str,
) -> Result<Vec<Judged<'a>>, Faults> {
    let key = format!("{}.{}", adapter.name_of_counter, invocation.name);
    let Some(rules) = dialects.find(&adapter.name_of_counter, &invocation.name) else {
        return Err(format!("{key} is a way of counting no dialect file describes").into());
    };
    let drift_is_judged =
        record.is_some_and(|record| is_same_build(&record.version, version_of_this_run));

    let mut prepared = Vec::with_capacity(corpus.cases.len());
    let mut faults = Vec::new();
    for case in &corpus.cases {
        let exception = record.and_then(|r| r.find_exception(&case.name, &invocation.name));
        let real = match exception {
            Some(exception) => exception.expected.clone(),
            None => match derive_answer(&case.truth, rules, &corpus.readings) {
                Ok(derivation) => derivation.real,
                Err(messages) => {
                    faults.extend(messages.iter().map(|m| format!("{}: {m}", case.name)));
                    continue;
                }
            },
        };
        let entry = record.and_then(|r| r.find(&case.name, &invocation.name));
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
        return Err(faults.into());
    }

    let files: Vec<&Path> = prepared.iter().map(|(case, ..)| case.input_file.as_path()).collect();
    let answers = measure_every_file(adapter, invocation, binary, &files);

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

// Answers come back in the order the files were given. Nearly all the time goes on waiting for
// somebody else's program to start and finish, so more run at once than the machine has cores.
fn measure_every_file(
    adapter: &Adapter,
    invocation: &Invocation,
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
                    let answered = adapter.measure(invocation, binary, file);
                    answers.lock().unwrap_or_else(|held| held.into_inner()).push((at, answered));
                }
            });
        }
    });
    let mut answers = answers.into_inner().unwrap_or_else(|held| held.into_inner());
    answers.sort_by_key(|(at, _)| *at);
    answers.into_iter().map(|(_, answered)| answered).collect()
}

/// Holds one answer against what the rules ask for. `live` is `None` for a counter that says there
/// is no such file.
///
/// The whole of both answers is compared, the stretches of other languages included, so a counter
/// that leaves [`Answer::regions`] empty fails every case holding another language even where all
/// its counts are right.
pub fn judge_conformance(real: &Answer, live: Option<&Answer>) -> Conformance {
    match live {
        None => Conformance::Unclaimed,
        Some(live) if live == real => Conformance::Agrees,
        Some(_) => Conformance::Fails,
    }
}

pub(crate) fn judge_drift(record: &RecordedAnswer, live: Option<&Answer>) -> Drift {
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
    use crate::truth::Truth;

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
    fn with_no_list_the_failure_that_fails_exactly_as_recorded_breaks_nothing() {
        let known = measured(Conformance::Fails, Some(Drift::Same));
        assert!(known.fails_exactly_as_recorded());
        assert!(known.is_a_failure());
        assert!(!breaks_the_run(known, None));

        let new = measured(Conformance::Fails, Some(Drift::Changed));
        assert!(!new.fails_exactly_as_recorded());
        assert!(breaks_the_run(new, None));

        assert!(breaks_the_run(measured(Conformance::Fails, None), None));

        let fixed = measured(Conformance::Agrees, Some(Drift::Changed));
        assert!(!fixed.is_a_failure());
        assert!(!breaks_the_run(fixed, None));
    }

    #[test]
    fn a_change_in_what_the_counter_claims_breaks_the_run() {
        let gone = measured(Conformance::Unclaimed, Some(Drift::NoLongerClaimed));
        assert!(gone.is_a_failure());
        assert!(breaks_the_run(gone, None));

        let appeared = measured(Conformance::Agrees, Some(Drift::NowClaimed));
        assert!(appeared.is_a_failure());
        assert!(breaks_the_run(appeared, None));

        let quiet = measured(Conformance::Unclaimed, None);
        assert!(!quiet.is_a_failure());
    }

    #[test]
    fn a_list_answers_for_every_failure_and_the_record_is_not_asked() {
        let named = KnownFailures::of("default:0400-a_case_built_by_a_test\n");
        let other = KnownFailures::of("9999-a_case_nobody_wrote\n");
        for drift in [Some(Drift::Same), Some(Drift::Changed), None] {
            let one = measured(Conformance::Fails, drift);
            assert!(!breaks_the_run(one, Some(&named)), "{drift:?} is named by the list");
            let same = measured(Conformance::Fails, drift);
            assert!(breaks_the_run(same, Some(&other)), "{drift:?} is not named by the list");
        }
        let broke = Judged {
            case: &a_case(),
            outcome: Outcome::Broke("it fell over".to_string()),
        };
        assert!(find_what_breaks_the_run(&[broke], "default", Some(&other)).len() == 1);
    }

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

    // The two lines derive as one comment and one code line, so a record holding those numbers
    // with no failure flag is a consistent one.
    fn judge_a_built_corpus(
        root: &std::path::Path,
        record_text: &str,
        dialects: &Dialects,
    ) -> Result<(), Faults> {
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

        let corpus = Corpus::read(&cases).unwrap_or_else(|faults| panic!("{faults:?}"));
        let record = RecordedAnswers::read(std::slice::from_ref(&recorded_dir), "tokei", dialects)
            .unwrap_or_else(|faults| panic!("{faults:?}"))
            .unwrap_or_else(|| panic!("no record was read"));
        let adapter = a_tokei_adapter();
        let judged = measure_and_judge_every_case(
            &adapter,
            &adapter.invocations[0],
            dialects,
            Path::new("a-binary-that-is-never-run"),
            &corpus,
            Some(&record),
            "tokei 14.0.0",
        );
        let _ = fs::remove_dir_all(root);
        judged.map(|_| ())
    }

    fn a_tokei_adapter() -> Adapter {
        Adapter {
            name_of_counter: "tokei".to_string(),
            repository: None,
            args: vec!["{file}".to_string()],
            explain_args: None,
            explain_output: None,
            explain_keep_from: None,
            version_flag: None,
            acquisition: None,
            invocations: vec![Invocation {
                name: "default".to_string(),
                args: Vec::new(),
                buckets: vec!["code".to_string(), "comments".to_string(), "blanks".to_string()],
                reader: Reader::Written(OutputFormat::TokeiJson),
            }],
        }
    }

    fn breaks_the_run(one: Measured<'_>, allowed: Option<&KnownFailures>) -> bool {
        let case = a_case();
        let judged = [Judged { case: &case, outcome: Outcome::Measured(one) }];
        !find_what_breaks_the_run(&judged, "default", allowed).is_empty()
    }

    fn a_case() -> Case {
        Case {
            name: "0400-a_case_built_by_a_test".to_string(),
            input_file: std::path::PathBuf::from("input.rs"),
            trap: String::new(),
            truth: Truth { lines: Vec::new() },
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
