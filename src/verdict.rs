use std::path::Path;

use crate::adapter::{Adapter, Dialect};
use crate::answer::Answer;
use crate::corpus::{AnswerBlock, Case, Corpus};

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Answers what its own definitions ask for, and the case file says the same.
    Agrees,
    /// Answers what its own definitions ask for, and the case file records a failure: it was fixed.
    Fixed,
    /// Answers something else, and the case file already carries that answer and a note.
    KnownFailure,
    /// Answers something else, and something other than what the case file recorded.
    NewFailure,
    /// Does not claim the file, and the case file says it does not.
    Unclaimed,
    /// Does not claim the file any more.
    NoLongerClaimed,
    /// Claims a file the case file says it does not.
    NowClaimed,
}

impl Verdict {
    /// A counter that stops claiming files, or starts claiming files a case says it does not, has
    /// changed what it counts, which is the one thing a suite of this kind exists to catch.
    pub fn is_a_failure(&self) -> bool {
        matches!(
            self,
            Verdict::NewFailure
                | Verdict::KnownFailure
                | Verdict::NoLongerClaimed
                | Verdict::NowClaimed
        )
    }

    pub fn breaks_the_run(&self) -> bool {
        self.is_a_failure() && *self != Verdict::KnownFailure
    }
}

/// One counter's answer to one case, beside what the case records and what the verdict was.
pub struct Judged<'a> {
    pub verdict: Verdict,
    pub case: &'a Case,
    pub answer: &'a AnswerBlock,
    pub live: Option<Answer>,
}

/// Runs the counter once per case, so this is as slow as the counter is, times the corpus. A case
/// that records no answer for this way of counting is passed over; a corpus where no case records
/// one is refused, since a counter nobody has written an answer for would otherwise be measured
/// against nothing and reported as agreeing on everything.
pub fn measure_and_judge_every_case<'a>(
    adapter: &Adapter,
    dialect: &Dialect,
    binary: &Path,
    corpus: &'a Corpus,
) -> Result<Vec<Judged<'a>>, String> {
    let mut judged = Vec::new();
    for case in &corpus.cases {
        let Some(answer) = case.find_answer_block(&adapter.name_of_counter, &dialect.name) else {
            continue;
        };
        let live = adapter.measure(dialect, binary, &case.input_file)?;
        judged.push(Judged { verdict: judge(answer, live.as_ref()), case, answer, live });
    }
    if judged.is_empty() {
        return Err(format!(
            "no case writes down an answer for {}.{}, so there was nothing to measure it against",
            adapter.name_of_counter, dialect.name
        ));
    }
    Ok(judged)
}

pub fn judge(answer: &AnswerBlock, live: Option<&Answer>) -> Verdict {
    match (&answer.counted, live) {
        (None, None) => Verdict::Unclaimed,
        (None, Some(_)) => Verdict::NowClaimed,
        (Some(_), None) => Verdict::NoLongerClaimed,
        (Some(counted), Some(live)) => match (answer.real == *live, counted == live) {
            (true, true) => Verdict::Agrees,
            (true, false) => Verdict::Fixed,
            (false, true) => Verdict::KnownFailure,
            (false, false) => Verdict::NewFailure,
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::adapter::Acquisition;
    use crate::answer::Counts;
    use crate::dialects::read_the_shipped_dialects;
    use crate::measurement::OutputFormat;

    // Every case answers all four ways of counting this suite knows, so the counter nobody has
    // answered is invented here, and nothing runs its binary because there is no case to run it on.
    #[test]
    fn a_way_of_counting_no_case_answers_is_refused_instead_of_agreeing_on_nothing() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("cases");
        let corpus =
            Corpus::read(&dir, &read_the_shipped_dialects()).unwrap_or_else(|faults| panic!("{faults:?}"));
        let unrecorded = Adapter {
            name_of_counter: "cloc".to_string(),
            output_format: OutputFormat::LinejudgeJson,
            args: vec!["{file}".to_string()],
            version_flag: None,
            acquisition: Acquisition {
                channel: "crates-io".to_string(),
                name: "cloc".to_string(),
            },
            dialects: vec![Dialect {
                name: "default".to_string(),
                args: Vec::new(),
                buckets: vec!["code".to_string(), "comments".to_string(), "blanks".to_string()],
            }],
        };
        let refused = measure_and_judge_every_case(
            &unrecorded,
            &unrecorded.dialects[0],
            Path::new("a-binary-that-is-never-run"),
            &corpus,
        )
        .err()
        .unwrap_or_else(|| panic!("a counter no case answers was reported on all the same"));
        assert!(refused.contains("cloc.default"), "{refused}");
    }

    #[test]
    fn a_counter_that_answers_what_the_case_expects_of_it_agrees() {
        let answer = an_answer(2, 2);
        assert_eq!(judge(&answer, Some(&a_measurement(2))), Verdict::Agrees);
    }

    #[test]
    fn a_recorded_failure_that_still_fails_the_same_way_is_known() {
        let answer = an_answer(2, 3);
        assert_eq!(judge(&answer, Some(&a_measurement(3))), Verdict::KnownFailure);
    }

    #[test]
    fn a_recorded_failure_that_has_stopped_failing_breaks_nothing() {
        let answer = an_answer(2, 3);
        let verdict = judge(&answer, Some(&a_measurement(2)));
        assert_eq!(verdict, Verdict::Fixed);
        assert!(!verdict.is_a_failure());
        assert!(!verdict.breaks_the_run());
    }

    #[test]
    fn a_counter_that_has_changed_what_it_claims_breaks_the_run_and_a_known_failure_does_not() {
        assert!(Verdict::NoLongerClaimed.breaks_the_run());
        assert!(Verdict::NowClaimed.breaks_the_run());
        assert!(Verdict::KnownFailure.is_a_failure());
        assert!(!Verdict::KnownFailure.breaks_the_run());
        assert!(!Verdict::Unclaimed.is_a_failure());
        assert!(!Verdict::Agrees.is_a_failure());
    }

    #[test]
    fn an_answer_nobody_wrote_down_is_the_one_that_breaks_the_run() {
        let answer = an_answer(2, 3);
        let verdict = judge(&answer, Some(&a_measurement(4)));
        assert_eq!(verdict, Verdict::NewFailure);
        assert!(verdict.breaks_the_run());
    }

    #[test]
    fn claiming_and_not_claiming_are_answers_of_their_own() {
        let claimed = an_answer(2, 2);
        let unclaimed = AnswerBlock {
            name_of_counter: "tokei".to_string(),
            dialect: "default".to_string(),
            real: a_measurement(2),
            counted: None,
            note: None,
        };
        assert_eq!(judge(&unclaimed, None), Verdict::Unclaimed);
        assert_eq!(judge(&claimed, None), Verdict::NoLongerClaimed);
        assert_eq!(judge(&unclaimed, Some(&a_measurement(2))), Verdict::NowClaimed);
    }

    fn an_answer(real_code: u32, counted_code: u32) -> AnswerBlock {
        AnswerBlock {
            name_of_counter: "tokei".to_string(),
            dialect: "default".to_string(),
            real: a_measurement(real_code),
            counted: Some(a_measurement(counted_code)),
            note: None,
        }
    }

    fn a_measurement(code: u32) -> Answer {
        Answer { counts: counts(code), regions: Vec::new() }
    }

    fn counts(code: u32) -> Counts {
        Counts {
            lines: 4,
            buckets: BTreeMap::from([
                ("code".to_string(), code),
                ("comments".to_string(), 4 - code),
                ("blanks".to_string(), 0),
            ]),
        }
    }
}
