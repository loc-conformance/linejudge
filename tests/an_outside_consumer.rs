use std::path::Path;

use linejudge::corpus::Corpus;
use linejudge::deriver::derive_answer;
use linejudge::dialects::Dialects;
use linejudge::recorded::{RecordedAnswers, is_same_build};
use linejudge::verdict::{Conformance, judge_conformance};

#[test]
fn a_consumer_judges_a_counter_over_the_whole_corpus_without_any_binary() {
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dialects = Dialects::read(&checkout.join("dialects"))
        .unwrap_or_else(|faults| panic!("{}", faults.join("\n")));
    let corpus =
        Corpus::read(&checkout.join("cases")).unwrap_or_else(|faults| panic!("{faults:?}"));
    assert!(corpus.cases.len() >= 80, "{} cases", corpus.cases.len());
    assert!(corpus.disabled.is_empty(), "{:?}", corpus.disabled);

    let rules = dialects.find("tokei", "default").unwrap_or_else(|| panic!("no tokei rules"));
    for case in &corpus.cases {
        assert!(!case.trap.trim().is_empty(), "{}", case.name);
        let real = derive_answer(&case.truth, rules, &corpus.readings)
            .unwrap_or_else(|faults| panic!("{}: {faults:?}", case.name))
            .real;
        assert_eq!(judge_conformance(&real, Some(&real)), Conformance::Agrees, "{}", case.name);
        assert_eq!(judge_conformance(&real, None), Conformance::Unclaimed, "{}", case.name);
        let mut wrong = real.clone();
        wrong.counts.lines += 1;
        assert_eq!(judge_conformance(&real, Some(&wrong)), Conformance::Fails, "{}", case.name);
    }
}

#[test]
fn a_consumer_reads_what_was_recorded_about_a_counter_of_the_roster() {
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dialects = Dialects::read(&checkout.join("dialects"))
        .unwrap_or_else(|faults| panic!("{}", faults.join("\n")));
    let record = RecordedAnswers::read(&checkout.join("recorded"), "tokei", &dialects)
        .unwrap_or_else(|faults| panic!("{}", faults.join("\n")))
        .unwrap_or_else(|| panic!("tokei has no recorded answers"));
    assert_eq!(record.counter, "tokei");
    assert!(!record.version.trim().is_empty());
    assert!(!is_same_build(&record.version, "unknown version"));

    let entry = record
        .find("3010-escaped_quote_before_comment", "default")
        .unwrap_or_else(|| panic!("0100 is not recorded"));
    let counted = entry.counted.as_ref().unwrap_or_else(|| panic!("0100 was not claimed"));
    assert!(counted.counts.buckets.contains_key("code"));
    assert!(!entry.is_known_failure);
    assert!(record.find_exception("3010-escaped_quote_before_comment", "default").is_none());
}
