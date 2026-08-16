use crate::corpus::Answer;
use crate::measurement::Measurement;

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

pub fn judge(answer: &Answer, live: Option<&Measurement>) -> Verdict {
    match (&answer.given, live) {
        (None, None) => Verdict::Unclaimed,
        (None, Some(_)) => Verdict::NowClaimed,
        (Some(_), None) => Verdict::NoLongerClaimed,
        (Some(given), Some(live)) => {
            let agrees = given.real == live.counts && given.real_regions == live.regions;
            let recorded = given.counted == live.counts && given.counted_regions == live.regions;
            match (agrees, recorded) {
                (true, true) => Verdict::Agrees,
                (true, false) => Verdict::Fixed,
                (false, true) => Verdict::KnownFailure,
                (false, false) => Verdict::NewFailure,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::corpus::{Counts, Recorded};

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
        let unclaimed = Answer {
            name_of_counter: "tokei".to_string(),
            dialect: "default".to_string(),
            given: None,
        };
        assert_eq!(judge(&unclaimed, None), Verdict::Unclaimed);
        assert_eq!(judge(&claimed, None), Verdict::NoLongerClaimed);
        assert_eq!(judge(&unclaimed, Some(&a_measurement(2))), Verdict::NowClaimed);
    }

    fn an_answer(real_code: u32, counted_code: u32) -> Answer {
        Answer {
            name_of_counter: "tokei".to_string(),
            dialect: "default".to_string(),
            given: Some(Recorded {
                real: counts(real_code),
                real_regions: Vec::new(),
                counted: counts(counted_code),
                counted_regions: Vec::new(),
                note: None,
                states_regions: false,
            }),
        }
    }

    fn a_measurement(code: u32) -> Measurement {
        Measurement { counts: counts(code), regions: Vec::new() }
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
