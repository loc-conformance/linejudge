use std::io::{self, Write};
use std::path::Path;

use linejudge::adapter::{Adapter, Invocation};
use linejudge::answer::{Counts, RegionCounts};
use linejudge::corpus::{Case, Corpus};
use linejudge::recorded::RecordedAnswers;
use linejudge::verdict::{
    Conformance, Drift, Judged, Measured, Outcome, find_what_breaks_the_run,
};

use crate::style;

const FILE_PLACEHOLDER: &str = "{file}";
const ROW_WIDTH: usize = "recorded note".len() + 1;
const REGION_ROW_WIDTH: usize = "recorded regions".len() + 1;

// One counter's one way of counting, as the report speaks about it: what ran, at which version,
// and whether a record of this build exists to hold the run against.
pub struct OneRun<'a> {
    pub adapter: &'a Adapter,
    pub dialect: &'a Invocation,
    pub binary: &'a Path,
    pub version: &'a str,
    pub drift_is_judged: bool,
}

// Returns whether what it found should break the run, which is any failure the record does not
// already hold.
pub fn report_the_verdicts_of_one_dialect(
    out: &mut dyn Write,
    run: &OneRun,
    judged: &[Judged],
) -> io::Result<bool> {
    let adapter = run.adapter;
    let dialect = run.dialect;
    let drift_is_judged = run.drift_is_judged;
    writeln!(out, "\n{}  {}",
            style::HEADING.paint(&format!("{}.{}", adapter.name_of_counter, dialect.name)),
            style::DETAIL.paint(&format!("[{}]", run.version)))?;
    writeln!(out, "  {}", format_summary(judged, drift_is_judged))?;
    // Once, and never per finding: the command is the same for every case but the file.
    writeln!(out, "  {} {}", style::LABEL.paint("run"), style::DETAIL.paint(
            &adapter.format_command(dialect, run.binary, Path::new(FILE_PLACEHOLDER))))?;

    for one in judged {
        let measured = match &one.outcome {
            Outcome::Broke(message) => {
                write_the_name_above_the_rows(out, &style::DIFFERS, "broke", &one.case.name)?;
                write_what_broke(out, message)?;
                continue;
            }
            Outcome::Measured(measured) => measured,
        };
        if measured.agrees_through_its_exception() && !measured.is_a_failure() {
            write_the_name_of(out, &style::AGREES, "agrees through its exception", &one.case.name)?;
            continue;
        }
        match (measured.conformance, measured.drift) {
            (Conformance::Fails, Some(Drift::Same)) => {}
            (Conformance::Fails, Some(Drift::Changed)) => {
                let what = match measured.record.is_some_and(|r| r.is_known_failure) {
                    true => "known, and it now fails in a new way",
                    false => "new failure",
                };
                write_the_name_above_the_rows(out, &style::DIFFERS, what, &one.case.name)?;
                describe(out, one.case, measured)?;
            }
            (Conformance::Fails, _) => {
                let what = if drift_is_judged { "new failure" } else { "fails" };
                write_the_name_above_the_rows(out, &style::DIFFERS, what, &one.case.name)?;
                describe(out, one.case, measured)?;
            }
            (_, Some(Drift::NoLongerClaimed)) => {
                write_the_name_above_the_rows(out, &style::RECORDED,
                        "claims the file no longer", &one.case.name)?;
                describe(out, one.case, measured)?;
            }
            (_, Some(Drift::NowClaimed)) => {
                write_the_name_above_the_rows(out, &style::RECORDED,
                        "claims a file the record says it does not", &one.case.name)?;
                describe(out, one.case, measured)?;
            }
            (Conformance::Agrees, Some(Drift::Changed)) => write_the_name_of(out, &style::AGREES,
                    "now agrees, the record still holds a failure", &one.case.name)?,
            _ => {}
        }
    }
    Ok(!find_what_breaks_the_run(judged).is_empty())
}

// An entry for a case that is neither judged nor disabled is left over from a rename or a removal,
// and is said out loud rather than silently never consulted.
pub fn report_recorded_answers_that_name_nothing(
    out: &mut dyn Write,
    record: &RecordedAnswers,
    corpus: &Corpus,
) -> io::Result<()> {
    for (case_name, dialect) in record.cases_spoken_about() {
        let known = corpus.cases.iter().any(|case| case.name == case_name)
            || corpus.disabled.iter().any(|name| name == case_name);
        if !known {
            write_the_name_of(out, &style::RECORDED, "recorded for a case that is not here",
                    &format!("{dialect}:{case_name}"))?;
        }
    }
    Ok(())
}

// Where another answer is given to hold these against, every number that differs is painted, so
// two rows of the same shape can be read by their colors alone.
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
    color: &style::Style,
    what: &str,
    name: &str,
) -> io::Result<()> {
    writeln!(out, "  {}   {name}", color.paint(what))
}

// A finding with rows under it opens with a blank line so it does not run into the one above. A
// finding that is only its name does not, so a run of them reads as the list it is.
fn write_the_name_above_the_rows(
    out: &mut dyn Write,
    color: &style::Style,
    what: &str,
    name: &str,
) -> io::Result<()> {
    writeln!(out)?;
    write_the_name_of(out, color, what, name)
}

fn write_row(out: &mut dyn Write, width: usize, name: &str, text: &str) -> io::Result<()> {
    let padding = " ".repeat(width.saturating_sub(name.chars().count()));
    writeln!(out, "      {}{padding}{text}", style::LABEL.paint(name))
}

fn describe(out: &mut dyn Write, case: &Case, measured: &Measured) -> io::Result<()> {
    let real = &measured.real;
    let recorded = measured.record.and_then(|record| record.counted.as_ref());
    match measured.record.and_then(|record| record.note.as_ref()) {
        Some(note) => write_row(out, ROW_WIDTH, "recorded note", &format_as_one_line(note))?,
        None => write_row(out, ROW_WIDTH, "trap", &format_as_one_line(&case.trap))?,
    }
    if let Some(exception) = measured.exception {
        write_row(out, ROW_WIDTH, "exception", &format_as_one_line(&exception.note))?;
    }
    match (&measured.live, recorded) {
        (Some(live), _) => {
            write_row(out, ROW_WIDTH, "wants", &paint_counts(&real.counts, None))?;
            if let Some(recorded) = recorded
                && recorded.counts != live.counts
            {
                write_row(out, ROW_WIDTH, "recorded",
                        &paint_counts(&recorded.counts, Some(&real.counts)))?;
            }
            if measured.record.is_some_and(|record| record.counted.is_none()) {
                writeln!(out, "      {}", style::DIFFERS.paint(
                        "the record holds no answer, since this counter claimed no such file"))?;
            }
            write_row(out, ROW_WIDTH, "answers", &paint_counts(&live.counts, Some(&real.counts)))?;
            if real.regions != live.regions {
                write_row(out, REGION_ROW_WIDTH, "wants regions", &format_regions(&real.regions))?;
                if let Some(recorded) = recorded
                    && recorded.regions != live.regions
                {
                    write_row(out, REGION_ROW_WIDTH, "recorded regions",
                            &format_regions(&recorded.regions))?;
                }
                write_row(out, REGION_ROW_WIDTH, "answers regions", &format_regions(&live.regions))?;
            }
        }
        (None, Some(recorded)) => {
            write_row(out, ROW_WIDTH, "wants", &paint_counts(&real.counts, None))?;
            write_row(out, ROW_WIDTH, "recorded", &paint_counts(&recorded.counts, Some(&real.counts)))?;
            writeln!(out, "      {}", style::DIFFERS.paint("answers nothing, it claims no such file"))?;
        }
        (None, None) => {
            write_row(out, ROW_WIDTH, "wants", &paint_counts(&real.counts, None))?;
            writeln!(out, "      {}", style::DIFFERS.paint("answers nothing, it claims no such file"))?;
        }
    }
    Ok(())
}

fn write_what_broke(out: &mut dyn Write, message: &str) -> io::Result<()> {
    writeln!(out, "      {}", format_as_one_line(message))
}

fn format_summary(judged: &[Judged], drift_is_judged: bool) -> String {
    let measured = |wanted: &dyn Fn(&Measured) -> bool| {
        judged
            .iter()
            .filter(|one| match &one.outcome {
                Outcome::Measured(measured) => wanted(measured),
                Outcome::Broke(_) => false,
            })
            .count()
    };
    let broke = judged.iter().filter(|one| matches!(one.outcome, Outcome::Broke(_))).count();
    let agree = measured(&|m| m.conformance == Conformance::Agrees);
    let fails = measured(&|m| m.conformance == Conformance::Fails);
    let known = measured(&|m| m.fails_exactly_as_recorded());
    let unclaimed = measured(&|m| m.conformance == Conformance::Unclaimed && m.drift != Some(Drift::NoLongerClaimed));
    // A count of none is left to fade, so what is there stands out from what is not.
    let painted = |count: usize, what: &str, color: style::Style| {
        let text = format!("{count} {what}");
        if count == 0 {
            style::DETAIL.paint(&text).to_string()
        } else {
            color.paint(&text).to_string()
        }
    };
    let mut said = match drift_is_judged {
        true => [
            painted(agree, "agree", style::AGREES),
            painted(known, "known failures", style::RECORDED),
            painted(fails - known, "new failures", style::DIFFERS),
            painted(unclaimed, "unclaimed", style::RECORDED),
        ]
        .join(", "),
        false => [
            painted(agree, "agree", style::AGREES),
            painted(fails, "fail", style::DIFFERS),
            painted(unclaimed, "unclaimed", style::RECORDED),
        ]
        .join(", "),
    };
    for (count, what, color) in [
        (broke, "broke", style::DIFFERS),
        (measured(&|m| m.agrees_through_its_exception()), "agreeing through an exception", style::RECORDED),
        (measured(&|m| m.conformance == Conformance::Agrees && m.drift == Some(Drift::Changed)),
                "fixed since the record was written", style::AGREES),
        (measured(&|m| m.drift == Some(Drift::NoLongerClaimed)), "no longer claimed", style::RECORDED),
        (measured(&|m| m.drift == Some(Drift::NowClaimed)), "claimed for the first time", style::RECORDED),
    ] {
        if count > 0 {
            said.push_str(&format!(", {}", painted(count, what, color)));
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
