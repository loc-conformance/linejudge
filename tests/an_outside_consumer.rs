use std::path::Path;

use linejudge::corpus::Corpus;
use linejudge::deriver::derive_answer;
use linejudge::dialects::Dialects;
use linejudge::recorded::{RecordedAnswers, is_same_build};
use linejudge::verdict::{Conformance, judge_conformance};

#[test]
fn a_consumer_judges_a_counter_over_the_whole_corpus_without_any_binary() {
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dialects = Dialects::read(&[checkout.join("dialects")])
        .unwrap_or_else(|faults| panic!("{}", faults.join("\n")));
    let corpus =
        Corpus::read(&checkout.join("cases")).unwrap_or_else(|faults| panic!("{faults:?}"));
    assert!(!corpus.cases.is_empty());

    let rules = dialects.find("tokei", "default").unwrap_or_else(|| panic!("no tokei rules"));
    let mut derived = Vec::new();
    for case in &corpus.cases {
        let answer = derive_answer(&case.truth, rules, &corpus.readings)
            .unwrap_or_else(|faults| panic!("{}: {faults:?}", case.name));
        derived.push(answer.real);
    }

    let real = &derived[0];
    let mut wrong = real.clone();
    wrong.counts.lines += 1;
    assert_eq!(judge_conformance(real, Some(real)), Conformance::Agrees);
    assert_eq!(judge_conformance(real, Some(&wrong)), Conformance::Fails);
    assert_eq!(judge_conformance(real, None), Conformance::Unclaimed);
}

#[test]
fn a_consumer_reads_what_was_recorded_about_a_counter_of_the_roster() {
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dialects = Dialects::read(&[checkout.join("dialects")])
        .unwrap_or_else(|faults| panic!("{}", faults.join("\n")));
    let record = RecordedAnswers::read(&[checkout.join("recorded")], "tokei", &dialects)
        .unwrap_or_else(|faults| panic!("{}", faults.join("\n")))
        .unwrap_or_else(|| panic!("tokei has no recorded answers"));
    assert_eq!(record.counter, "tokei");
    assert!(is_same_build(&record.version, &record.version));

    let spoken: Vec<(String, String)> = record
        .cases_spoken_about()
        .map(|(case, dialect)| (case.to_string(), dialect.to_string()))
        .collect();
    assert!(!spoken.is_empty(), "the record speaks about no case");
    for (name_of_case, name_of_dialect) in &spoken {
        let entry = record
            .find(name_of_case, name_of_dialect)
            .unwrap_or_else(|| panic!("{name_of_case} is not recorded"));
        if let Some(counted) = &entry.counted {
            assert!(counted.counts.buckets.contains_key("code"), "{name_of_case}");
        }
        // Nothing here declares one, which is what CONTRIBUTING says of this repository.
        assert!(record.find_exception(name_of_case, name_of_dialect).is_none(), "{name_of_case}");
    }
}
