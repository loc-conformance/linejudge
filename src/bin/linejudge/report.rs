use std::io::{self, Write};
use std::path::Path;

use linejudge::adapter::{Adapter, Dialect};
use linejudge::corpus::Corpus;
use linejudge::answer::{Counts, RegionCounts};
use linejudge::known_failures::KnownFailures;
use linejudge::verdict::{Judged, Verdict};

// Returns whether what it found should break the run: without a list of known failures that is
// any failure the corpus does not already record, and with one it is any failure the list does
// not name.
pub fn report_the_verdicts_of_one_dialect(
    out: &mut dyn Write,
    adapter: &Adapter,
    dialect: &Dialect,
    binary: &Path,
    version: &str,
    judged: &[Judged],
    known_failures: Option<&KnownFailures>,
) -> io::Result<bool> {
    writeln!(out, "\n{}.{}  [{version}]", adapter.name_of_counter, dialect.name)?;
    writeln!(out, "  {}", format_summary(judged))?;

    let Some(known_failures) = known_failures else {
        for one in judged {
            match one.verdict {
                Verdict::NewFailure => {
                    writeln!(out, "  new failure   {}", one.case.name)?;
                    describe(out, adapter, dialect, binary, one)?;
                }
                Verdict::NoLongerClaimed => {
                    writeln!(out, "  claims the file no longer   {}", one.case.name)?;
                    describe(out, adapter, dialect, binary, one)?;
                }
                Verdict::NowClaimed => {
                    writeln!(out, "  claims a file the case says it does not   {}", one.case.name)?;
                    describe(out, adapter, dialect, binary, one)?;
                }
                Verdict::Fixed => writeln!(
                    out,
                    "  now agrees, the case still records a failure   {}",
                    one.case.name
                )?,
                _ => {}
            }
        }
        return Ok(judged.iter().any(|one| one.verdict.breaks_the_run()));
    };

    let mut unnamed = 0;
    for one in judged {
        let named = known_failures.names(&dialect.name, &one.case.number);
        match (one.verdict.is_a_failure(), named) {
            (true, false) => {
                unnamed += 1;
                writeln!(out, "  fails and is not a known failure   {}", one.case.name)?;
                describe(out, adapter, dialect, binary, one)?;
            }
            (true, true) if one.verdict == Verdict::NewFailure => {
                writeln!(out, "  known, and it now fails in a new way   {}", one.case.name)?;
                describe(out, adapter, dialect, binary, one)?;
            }
            (false, true) => writeln!(out, "  passes, take it off the list   {}", one.case.name)?,
            _ => {}
        }
    }
    Ok(unnamed > 0)
}

pub fn report_entries_that_name_nothing(
    out: &mut dyn Write,
    adapter: &Adapter,
    corpus: &Corpus,
    known_failures: &KnownFailures,
) -> io::Result<()> {
    for (dialect, number) in known_failures.entries() {
        let dialect_is_real =
            dialect.is_none_or(|d| adapter.dialects.iter().any(|one| one.name == d));
        let case_is_real = corpus.cases.iter().any(|case| case.number == number);
        if !dialect_is_real || !case_is_real {
            let entry = match dialect {
                Some(dialect) => format!("{dialect}:{number}"),
                None => number.to_string(),
            };
            writeln!(out, "  names nothing, take it off the list   {entry}")?;
        }
    }
    Ok(())
}

fn describe(
    out: &mut dyn Write,
    adapter: &Adapter,
    dialect: &Dialect,
    binary: &Path,
    one: &Judged,
) -> io::Result<()> {
    let real = &one.answer.real;
    match (&one.answer.counted, &one.live) {
        (Some(_), Some(live)) => {
            match &one.answer.note {
                Some(note) => writeln!(out, "      note    {}", format_as_one_line(note))?,
                None => writeln!(out, "      trap    {}", format_as_one_line(&one.case.trap))?,
            }
            writeln!(out, "      wants   {}", format_counts(&real.counts))?;
            writeln!(out, "      answers {}", format_counts(&live.counts))?;
            if real.regions != live.regions {
                writeln!(out, "      wants regions   {}", format_regions(&real.regions))?;
                writeln!(out, "      answers regions {}", format_regions(&live.regions))?;
            }
        }
        (Some(_), None) => {
            writeln!(out, "      wants   {}", format_counts(&real.counts))?;
            writeln!(out, "      answers nothing, it claims no such file")?;
        }
        (None, Some(live)) => {
            writeln!(out, "      the case records no answer, since this counter claimed no such file")?;
            writeln!(out, "      answers {}", format_counts(&live.counts))?;
        }
        (None, None) => return Ok(()),
    }
    writeln!(out, "      run     {}", adapter.format_command(dialect, binary, &one.case.input_file))
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
        .map(|region| {
            let lines = if region.lines == 1 { "line" } else { "lines" };
            format!("{} of {} {lines}", region.language, region.lines)
        })
        .collect::<Vec<_>>()
        .join(", ")
}
