use std::path::Path;

use linejudge::adapter::{Adapter, Dialect};
use linejudge::corpus::{Answer, Case, Corpus, Counts, RegionCounts};
use linejudge::known_failures::KnownFailures;
use linejudge::measurement::Measurement;
use linejudge::verdict::{Verdict, judge};

// One counter's answer to one case, beside what the case records and what the verdict was.
pub struct Judged<'a> {
    pub verdict: Verdict,
    pub case: &'a Case,
    pub answer: &'a Answer,
    pub live: Option<Measurement>,
}

// Returns whether what it found should break the run: without a list of known failures that is
// any failure the corpus does not already record, and with one it is any failure the list does
// not name.
pub fn report_the_verdicts_of_one_dialect(
    adapter: &Adapter,
    dialect: &Dialect,
    binary: &Path,
    version: &str,
    corpus: &Corpus,
    known_failures: Option<&KnownFailures>,
) -> Result<bool, String> {
    let mut judged: Vec<Judged> = Vec::new();
    for case in &corpus.cases {
        let Some(answer) = case.find_answer(&adapter.name_of_counter, &dialect.name) else {
            continue;
        };
        let live: Option<Measurement> = adapter.measure(dialect, binary, &case.input)?;
        judged.push(Judged { verdict: judge(answer, live.as_ref()), case, answer, live });
    }

    println!("\n{}.{}  [{version}]", adapter.name_of_counter, dialect.name);
    println!("  {}", format_summary(&judged));
    let told = |one: &Judged| describe(adapter, dialect, binary, one);

    let Some(known_failures) = known_failures else {
        for one in &judged {
            match one.verdict {
                Verdict::NewFailure => {
                    println!("  new failure   {}", one.case.name);
                    told(one);
                }
                Verdict::NoLongerClaimed => {
                    println!("  claims the file no longer   {}", one.case.name);
                    told(one);
                }
                Verdict::NowClaimed => {
                    println!("  claims a file the case says it does not   {}", one.case.name);
                    told(one);
                }
                Verdict::Fixed => {
                    println!("  now agrees, the case still records a failure   {}", one.case.name)
                }
                _ => {}
            }
        }
        return Ok(judged.iter().any(|one| one.verdict.breaks_the_run()));
    };

    let mut unnamed = 0;
    for one in &judged {
        let named = known_failures.names(&dialect.name, &one.case.number);
        match (one.verdict.is_a_failure(), named) {
            (true, false) => {
                unnamed += 1;
                println!("  fails and is not a known failure   {}", one.case.name);
                told(one);
            }
            (true, true) if one.verdict == Verdict::NewFailure => {
                println!("  known, and it now fails in a new way   {}", one.case.name);
                told(one);
            }
            (false, true) => println!("  passes, take it off the list   {}", one.case.name),
            _ => {}
        }
    }
    Ok(unnamed > 0)
}

pub fn report_entries_that_name_nothing(
    adapter: &Adapter,
    corpus: &Corpus,
    known_failures: &KnownFailures,
) {
    for (dialect, number) in known_failures.entries() {
        let dialect_is_real =
            dialect.is_none_or(|d| adapter.dialects.iter().any(|one| one.name == d));
        let case_is_real = corpus.cases.iter().any(|case| case.number == number);
        if !dialect_is_real || !case_is_real {
            let entry = match dialect {
                Some(dialect) => format!("{dialect}:{number}"),
                None => number.to_string(),
            };
            println!("  names nothing, take it off the list   {entry}");
        }
    }
}

fn describe(adapter: &Adapter, dialect: &Dialect, binary: &Path, one: &Judged) {
    match (&one.answer.given, &one.live) {
        (Some(recorded), Some(live)) => {
            match &recorded.note {
                Some(note) => println!("      note    {}", format_as_one_line(note)),
                None => println!("      trap    {}", format_as_one_line(&one.case.trap)),
            }
            println!("      wants   {}", format_counts(&recorded.real));
            println!("      answers {}", format_counts(&live.counts));
            if recorded.real_regions != live.regions {
                println!("      wants regions   {}", format_regions(&recorded.real_regions));
                println!("      answers regions {}", format_regions(&live.regions));
            }
        }
        (Some(recorded), None) => {
            println!("      wants   {}", format_counts(&recorded.real));
            println!("      answers nothing, it claims no such file");
        }
        (None, Some(live)) => {
            println!("      the case records no answer, since this counter claimed no such file");
            println!("      answers {}", format_counts(&live.counts));
        }
        (None, None) => return,
    }
    println!("      run     {}", adapter.format_command(dialect, binary, &one.case.input));
}

fn format_summary(judged: &[Judged]) -> String {
    let of = |wanted: Verdict| judged.iter().filter(|one| one.verdict == wanted).count();
    let mut said = format!(
        "{} agree, {} known failures, {} new failures, {} unclaimed",
        of(Verdict::Agrees),
        of(Verdict::KnownFailure),
        of(Verdict::NewFailure),
        of(Verdict::Unclaimed),
    );
    for (count, what) in [
        (of(Verdict::Fixed), "fixed since the case was written"),
        (of(Verdict::NoLongerClaimed), "no longer claimed"),
        (of(Verdict::NowClaimed), "claimed for the first time"),
    ] {
        if count > 0 {
            said.push_str(&format!(", {count} {what}"));
        }
    }
    said
}

fn format_as_one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn format_counts(counts: &Counts) -> String {
    let mut named = vec![format!("{} lines", counts.lines)];
    named.extend(counts.buckets.iter().map(|(name, value)| format!("{value} {name}")));
    named.join(", ")
}

fn format_regions(regions: &[RegionCounts]) -> String {
    if regions.is_empty() {
        return "none".to_string();
    }
    regions
        .iter()
        .map(|region| format!("{} of {} lines", region.language, region.lines))
        .collect::<Vec<_>>()
        .join(", ")
}
