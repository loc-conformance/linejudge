use std::io::{self, Write};
use std::path::Path;

use linejudge::adapter::{Adapter, Dialect};
use linejudge::answer::{Counts, RegionCounts};
use linejudge::corpus::Corpus;
use linejudge::known_failures::KnownFailures;
use linejudge::verdict::{Judged, Verdict};

use crate::style;

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
    writeln!(out, "\n{}  {}",
            style::HEADING.paint(&format!("{}.{}", adapter.name_of_counter, dialect.name)),
            style::DETAIL.paint(&format!("[{version}]")))?;
    writeln!(out, "  {}", format_summary(judged))?;

    let Some(known_failures) = known_failures else {
        for one in judged {
            match one.verdict {
                Verdict::NewFailure => {
                    write_the_name_of(out, &style::DIFFERS, "new failure", &one.case.name)?;
                    describe(out, adapter, dialect, binary, one)?;
                }
                Verdict::NoLongerClaimed => {
                    write_the_name_of(out, &style::RECORDED, "claims the file no longer", &one.case.name)?;
                    describe(out, adapter, dialect, binary, one)?;
                }
                Verdict::NowClaimed => {
                    write_the_name_of(out, &style::RECORDED,
                            "claims a file the case says it does not", &one.case.name)?;
                    describe(out, adapter, dialect, binary, one)?;
                }
                Verdict::Fixed => write_the_name_of(out, &style::AGREES,
                        "now agrees, the case still records a failure", &one.case.name)?,
                _ => {}
            }
        }
        return Ok(judged.iter().any(|one| one.verdict.breaks_the_run()));
    };

    let mut unnamed = 0;
    for one in judged {
        let named = known_failures.names(&dialect.name, &one.case.name);
        match (one.verdict.is_a_failure(), named) {
            (true, false) => {
                unnamed += 1;
                write_the_name_of(out, &style::DIFFERS,
                        "fails and is not a known failure", &one.case.name)?;
                describe(out, adapter, dialect, binary, one)?;
            }
            (true, true) if one.verdict == Verdict::NewFailure => {
                write_the_name_of(out, &style::DIFFERS,
                        "known, and it now fails in a new way", &one.case.name)?;
                describe(out, adapter, dialect, binary, one)?;
            }
            (false, true) => write_the_name_of(out, &style::AGREES,
                    "passes, take it off the list", &one.case.name)?,
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
    for (dialect, case_name) in known_failures.entries() {
        let dialect_is_real =
            dialect.is_none_or(|d| adapter.dialects.iter().any(|one| one.name == d));
        let case_is_real = corpus.cases.iter().any(|case| case.name == case_name);
        if !dialect_is_real || !case_is_real {
            let entry = match dialect {
                Some(dialect) => format!("{dialect}:{case_name}"),
                None => case_name.to_string(),
            };
            write_the_name_of(out, &style::RECORDED, "names nothing, take it off the list", &entry)?;
        }
    }
    Ok(())
}

/// The counts of one answer, the numbers apart from the buckets they are counts of. Where another
/// answer is given to hold them against, every number that differs from it is painted as the
/// difference it is, so that two rows of the same shape can be read by their colours alone.
pub fn paint_counts(counts: &Counts, against: Option<&Counts>) -> String {
    let mut named = vec![paint_one_count(counts.lines, "lines",
            against.is_some_and(|other| other.lines != counts.lines))];
    named.extend(counts.buckets.iter().map(|(name, value)| {
        let differs = against
            .is_some_and(|other| other.buckets.get(name).is_none_or(|theirs| theirs != value));
        paint_one_count(*value, name, differs)
    }));
    named.join(", ")
}

fn write_the_name_of(
    out: &mut dyn Write,
    ink: &style::Style,
    what: &str,
    name: &str,
) -> io::Result<()> {
    writeln!(out, "  {}   {name}", ink.paint(what))
}

fn describe(
    out: &mut dyn Write,
    adapter: &Adapter,
    dialect: &Dialect,
    binary: &Path,
    one: &Judged,
) -> io::Result<()> {
    let real = &one.answer.real;
    let label = |text: &str| style::LABEL.paint(text).to_string();
    match (&one.answer.counted, &one.live) {
        (Some(_), Some(live)) => {
            match &one.answer.note {
                Some(note) => writeln!(out, "      {}    {}", label("note"), format_as_one_line(note))?,
                None => writeln!(out, "      {}    {}", label("trap"), format_as_one_line(&one.case.trap))?,
            }
            writeln!(out, "      {}   {}", label("wants"), paint_counts(&real.counts, None))?;
            writeln!(out, "      {} {}", label("answers"), paint_counts(&live.counts, Some(&real.counts)))?;
            if real.regions != live.regions {
                writeln!(out, "      {}   {}", label("wants regions"), format_regions(&real.regions))?;
                writeln!(out, "      {} {}", label("answers regions"), format_regions(&live.regions))?;
            }
        }
        (Some(_), None) => {
            writeln!(out, "      {}   {}", label("wants"), paint_counts(&real.counts, None))?;
            writeln!(out, "      {}", style::DIFFERS.paint("answers nothing, it claims no such file"))?;
        }
        (None, Some(live)) => {
            writeln!(out, "      {}", style::DIFFERS.paint(
                    "the case records no answer, since this counter claimed no such file"))?;
            writeln!(out, "      {} {}", label("answers"), paint_counts(&live.counts, None))?;
        }
        (None, None) => return Ok(()),
    }
    writeln!(out, "      {}     {}", label("run"), style::DETAIL.paint(
            &adapter.format_command(dialect, binary, &one.case.input_file)))
}

fn format_summary(judged: &[Judged]) -> String {
    let of = |wanted: Verdict| judged.iter().filter(|one| one.verdict == wanted).count();
    // A count of none says nothing and is left to fade, so that what is there stands out from what
    // is not.
    let painted = |count: usize, what: &str, ink: style::Style| {
        let text = format!("{count} {what}");
        if count == 0 { style::DETAIL.paint(&text).to_string() } else { ink.paint(&text).to_string() }
    };
    let mut said = [
        painted(of(Verdict::Agrees), "agree", style::AGREES),
        painted(of(Verdict::KnownFailure), "known failures", style::RECORDED),
        painted(of(Verdict::NewFailure), "new failures", style::DIFFERS),
        painted(of(Verdict::Unclaimed), "unclaimed", style::RECORDED),
    ]
    .join(", ");
    for (count, what, style) in [
        (of(Verdict::Fixed), "fixed since the case was written", style::AGREES),
        (of(Verdict::NoLongerClaimed), "no longer claimed", style::RECORDED),
        (of(Verdict::NowClaimed), "claimed for the first time", style::RECORDED),
    ] {
        if count > 0 {
            said.push_str(&format!(", {}", painted(count, what, style)));
        }
    }
    said
}

fn paint_one_count(value: u32, name: &str, differs: bool) -> String {
    if differs {
        return style::DIFFERS.paint(&format!("{value} {name}")).to_string();
    }
    format!("{} {}", style::NUMBER.paint(&value.to_string()), style::LABEL.paint(name))
}

fn format_as_one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn format_regions(regions: &[RegionCounts]) -> String {
    if regions.is_empty() {
        return style::DETAIL.paint("none").to_string();
    }
    regions
        .iter()
        .map(|region| {
            let lines = if region.lines == 1 { "line" } else { "lines" };
            format!("{} {} {}", style::REGION.paint(&region.language), style::DETAIL.paint("of"),
                    paint_one_count(region.lines, lines, false))
        })
        .collect::<Vec<_>>()
        .join(", ")
}
