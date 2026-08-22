use std::io::{self, Write};
use std::path::Path;

use linejudge::adapter::{Adapter, Invocation};
use linejudge::answer::{Answer, Counts, RegionCounts};
use linejudge::corpus::{Case, Corpus};
use linejudge::deriver::{ExplainedLine, derive_answer, explain_every_line};
use linejudge::dialects::Dialects;
use linejudge::per_line::{PerLineAnswer, PerLineFormat, read_per_line};
use linejudge::readings::Readings;
use linejudge::truth::{Covering, TruthLine};

use crate::report::paint_counts;
use crate::style;

const BY_ITS_RULES: &str = "by its rules";
const IN_ITS_REGIONS: &str = "in its regions";

pub fn find_case<'c>(corpus: &'c Corpus, name: &str) -> Result<&'c Case, String> {
    corpus.find_case(name).map_err(|fitting| match fitting.as_slice() {
        [] => format!("no case is named {name}"),
        several => {
            let named: Vec<&str> = several.iter().map(|case| case.name.as_str()).collect();
            format!(
                "no case is named {name}, and more than one contains it:\n  {}",
                named.join("\n  ")
            )
        }
    })
}

// One block per way this counter counts: every line of the case beside its marked spans, the rule
// that took it and the predicates that hold on it, and whatever the counter itself says about the
// file line by line.
pub fn explain_one_counter(
    out: &mut dyn Write,
    adapter: &Adapter,
    binary: Option<&Path>,
    case: &Case,
    dialects: &Dialects,
    readings: &Readings,
) -> io::Result<()> {
    for way in &adapter.invocations {
        let block = OneWay { counter: &adapter.name_of_counter, way: &way.name, case };
        let Some(dialect) = dialects.find(&adapter.name_of_counter, &way.name) else {
            writeln!(out, "\n{} on {}: no dialect file, nothing to derive with",
                    block.key(), case.name)?;
            continue;
        };
        let derived = derive_answer(&case.truth, dialect, readings)
            .and_then(|derivation| Ok((derivation.real, explain_every_line(&case.truth, dialect, readings)?)));
        let (real, explained) = match derived {
            Ok(both) => both,
            Err(faults) => {
                writeln!(out, "\n{} on {}:", block.key(), case.name)?;
                for fault in faults.iter() {
                    writeln!(out, "  {}", style::DIFFERS.paint(fault))?;
                }
                continue;
            }
        };
        let theirs = read_what_the_counter_says(adapter, way, binary, case);
        let its = run_the_counter(adapter, way, binary, case);
        write_the_header(out, &block, &real, &its, &theirs, &explained)?;
        write_every_line(out, &block, &explained, &theirs)?;
        if let (TheirAnswer::Text(printed), Some(binary)) = (&theirs, binary) {
            if let Some(command) = adapter.format_explain_command(way, binary, &case.input_file) {
                writeln!(out, "\n  {} {}",
                        style::LABEL.paint(&format!("what {} itself says, from", adapter.name_of_counter)),
                        style::DETAIL.paint(&command))?;
            }
            write_what_it_printed(out, printed, adapter.explain_keep_from.as_deref())?;
        }
    }
    Ok(())
}

struct OneWay<'a> {
    counter: &'a str,
    way: &'a str,
    case: &'a Case,
}

impl OneWay<'_> {
    fn key(&self) -> String {
        format!("{}.{}", self.counter, self.way)
    }
}

// What the counter answers when it is simply run, which is the answer `check` judges. Kept apart
// from its line by line analysis, since a counter that has none still answers.
enum ItsAnswer {
    NotMeasured,
    Broke(String),
    // It says there is no such file, which is an answer of its own and never a failure.
    Unclaimed,
    Counted(Answer),
}

// What the counter itself said about the file: nothing for one that declares no per-line command,
// and text for a person where it declares no format to read.
enum TheirAnswer {
    NoCommand,
    NoBinary,
    Broken(String),
    Unreadable(String),
    PerLine(PerLineAnswer),
    Text(String),
}

impl TheirAnswer {
    fn count_lines_read_differently(&self, explained: &[ExplainedLine]) -> usize {
        let TheirAnswer::PerLine(answer) = self else { return 0 };
        explained
            .iter()
            .zip(&answer.buckets_of_lines)
            .filter(|(ours, theirs)| ours.bucket != **theirs)
            .count()
    }

    fn find_bucket_of_line(&self, at: usize) -> Option<&str> {
        match self {
            TheirAnswer::PerLine(answer) => answer.buckets_of_lines.get(at).map(String::as_str),
            _ => None,
        }
    }
}

fn run_the_counter(adapter: &Adapter, way: &Invocation, binary: Option<&Path>, case: &Case) -> ItsAnswer {
    let Some(binary) = binary else { return ItsAnswer::NotMeasured };
    match adapter.measure(way, binary, &case.input_file) {
        Ok(Some(answer)) => ItsAnswer::Counted(answer),
        Ok(None) => ItsAnswer::Unclaimed,
        Err(message) => ItsAnswer::Broke(message),
    }
}

fn read_what_the_counter_says(
    adapter: &Adapter,
    way: &Invocation,
    binary: Option<&Path>,
    case: &Case,
) -> TheirAnswer {
    if adapter.explain_args.is_none() {
        return TheirAnswer::NoCommand;
    }
    let Some(binary) = binary else { return TheirAnswer::NoBinary };
    let printed = match adapter.run_explain(way, binary, &case.input_file) {
        Some(Ok(printed)) => printed,
        Some(Err(message)) => return TheirAnswer::Broken(message),
        None => return TheirAnswer::NoCommand,
    };
    match adapter.explain_output {
        None => TheirAnswer::Text(printed),
        Some(PerLineFormat::LinejudgePerLine) => {
            match read_per_line(&printed, &way.buckets, case.truth.lines.len()) {
                Ok(answer) => TheirAnswer::PerLine(answer),
                Err(message) => TheirAnswer::Unreadable(message),
            }
        }
    }
}

// The answer shown is the plain run, the one `check` judges, so a counter with nothing to say line
// by line is still held to its own rules here instead of showing a derivation and no verdict.
fn write_the_header(
    out: &mut dyn Write,
    block: &OneWay,
    real: &Answer,
    its: &ItsAnswer,
    theirs: &TheirAnswer,
    explained: &[ExplainedLine],
) -> io::Result<()> {
    let counter = block.counter;
    let answers = format!("{counter} answers");
    let width = answers
        .chars()
        .count()
        .max(BY_ITS_RULES.chars().count())
        .max(IN_ITS_REGIONS.chars().count());
    writeln!(out, "\n{} {} {}", style::HEADING.paint(&block.key()), style::DETAIL.paint("on"),
            style::HEADING.paint(&block.case.name))?;
    write_a_row(out, width, BY_ITS_RULES, &paint_counts(&real.counts, None))?;

    match its {
        ItsAnswer::NotMeasured => {
            let said = match theirs {
                TheirAnswer::NoBinary => format!(
                    "no binary named for {counter}, so neither what it answers nor how it reads \
                     the lines was measured"),
                _ => format!("no binary named for {counter}, so what it answers was not measured"),
            };
            writeln!(out, "  {}", style::DETAIL.paint(&said))?;
        }
        ItsAnswer::Broke(message) => writeln!(out, "  {} {}",
                style::DIFFERS.paint(&format!("{counter} could not be run:")),
                style::DETAIL.paint(message))?,
        ItsAnswer::Unclaimed => write_a_row(out, width, &answers,
                &style::RECORDED.paint("nothing, it claims no such file").to_string())?,
        ItsAnswer::Counted(answer) => {
            let mark = match answer == real {
                true => style::AGREES.paint("✓ agrees"),
                false => style::DIFFERS.paint("✗ differs"),
            };
            write_a_row(out, width, &answers,
                    &format!("{}   {mark}", paint_counts(&answer.counts, Some(&real.counts))))?;
            if answer.regions != real.regions {
                write_the_regions_that_differ(out, width, &real.regions, &answer.regions)?;
            }
        }
    }

    match theirs {
        TheirAnswer::NoCommand => writeln!(out, "  {}", style::DETAIL.paint(
                &format!("{counter} declares no per-line command of its own")),),
        // The line above has already said that nothing of this counter was measured.
        TheirAnswer::NoBinary => Ok(()),
        TheirAnswer::Broken(message) => writeln!(out, "  {} {}",
                style::DIFFERS.paint(&format!("the per-line command of {counter} could not be run:")),
                style::DETAIL.paint(message)),
        TheirAnswer::Unreadable(message) => writeln!(out, "  {} {}",
                style::DIFFERS.paint(&format!("what {counter} printed could not be read:")),
                style::DETAIL.paint(message)),
        TheirAnswer::Text(_) => Ok(()),
        TheirAnswer::PerLine(_) => {
            let how = match theirs.count_lines_read_differently(explained) {
                0 => style::DETAIL.paint(&format!("{counter} reads every line the same way")),
                1 => style::DIFFERS.paint(&format!("{counter} reads 1 line differently")),
                many => style::DIFFERS.paint(&format!("{counter} reads {many} lines differently")),
            };
            writeln!(out, "  {how}")
        }
    }
}

fn write_a_row(out: &mut dyn Write, width: usize, label: &str, text: &str) -> io::Result<()> {
    writeln!(out, "  {}  {text}", style::LABEL.paint(&format!("{label:<width$}")))
}

// Two answers can name the same languages over the same lines and still differ, by binning those
// lines differently, so what is shown is the counts and not the names.
fn write_the_regions_that_differ(
    out: &mut dyn Write,
    width: usize,
    real: &[RegionCounts],
    its: &[RegionCounts],
) -> io::Result<()> {
    let mut languages: Vec<&str> =
        real.iter().chain(its).map(|region| region.language.as_str()).collect();
    languages.sort_unstable();
    languages.dedup();
    let mut label = IN_ITS_REGIONS;
    for language in languages {
        let wanted = find_the_region_of(real, language);
        let found = find_the_region_of(its, language);
        if wanted == found {
            continue;
        }
        let against = match (wanted, found) {
            (Some(wanted), Some(found)) => format!("{} {} {}",
                    paint_counts(&count_the_lines_of(wanted), None), style::DETAIL.paint("against"),
                    paint_counts(&count_the_lines_of(found), Some(&count_the_lines_of(wanted)))),
            (Some(wanted), None) => format!("{} {} {}",
                    paint_counts(&count_the_lines_of(wanted), None), style::DETAIL.paint("against"),
                    style::DIFFERS.paint("none")),
            (None, Some(found)) => format!("{} {} {}", style::DETAIL.paint("none"),
                    style::DETAIL.paint("against"),
                    paint_counts(&count_the_lines_of(found), None)),
            (None, None) => continue,
        };
        write_a_row(out, width, label, &format!("{}  {against}", style::REGION.paint(language)))?;
        label = "";
    }
    Ok(())
}

fn find_the_region_of<'a>(regions: &'a [RegionCounts], language: &str) -> Option<&'a RegionCounts> {
    regions.iter().find(|region| region.language == language)
}

fn count_the_lines_of(region: &RegionCounts) -> Counts {
    Counts { lines: region.lines, buckets: region.buckets.clone() }
}

// What the counter itself says is written under a line only where it differs from the rules.
fn write_every_line(
    out: &mut dyn Write,
    block: &OneWay,
    explained: &[ExplainedLine],
    theirs: &TheirAnswer,
) -> io::Result<()> {
    let case = block.case;
    let counter = block.counter;
    let width = case.truth.lines.len().to_string().len();
    for (at, (line, truth_line)) in explained.iter().zip(&case.truth.lines).enumerate() {
        writeln!(out)?;
        let source = paint_by_marks(truth_line);
        writeln!(out, "  {:>width$}  {source}", at + 1)?;
        if !truth_line.marker.trim().is_empty() {
            writeln!(out, "  {:>width$}  {}", "", style::DETAIL.paint(&truth_line.marker))?;
        }
        let named: Vec<String> =
                line.rules.iter().map(|rule| style::RULE.paint(rule).to_string()).collect();
        let mut verdict = format!("{}  {} {}", style::LABEL.paint(&line.bucket),
                style::DETAIL.paint("by"),
                named.join(&format!(" {} ", style::DETAIL.paint("and by"))));
        if !line.holds.is_empty() {
            verdict.push_str(&format!("   {}", style::DETAIL.paint(&format!("({})", line.holds.join(", ")))));
        }
        if let Some(region) = &line.region {
            verdict.push_str(&format!("   {} {}", style::DETAIL.paint("in"), style::REGION.paint(region)));
        }
        writeln!(out, "  {:>width$}  {verdict}", "")?;
        if let Some(bucket) = theirs.find_bucket_of_line(at)
            && bucket != line.bucket
        {
            writeln!(out, "  {:>width$}  {}", "",
                    style::DIFFERS.paint(&format!("{counter} says {bucket}")))?;
        }
    }
    Ok(())
}

// Shows the lines holding the declared text, each from that text to its end. Lines are chosen and
// cut, never read. Where nothing holds it, everything is shown and this says so, so a tool that
// changed what it prints falls back to the whole of it instead of to silence.
fn write_what_it_printed(
    out: &mut dyn Write,
    printed: &str,
    keep_from: Option<&str>,
) -> io::Result<()> {
    if let Some(text) = keep_from {
        let kept: Vec<&str> = printed
            .lines()
            .filter_map(|line| line.find(text).map(|at| &line[at..]))
            .collect();
        if kept.is_empty() {
            writeln!(out, "    {}", style::DIFFERS.paint(&format!(
                    "nothing it printed holds [{text}], so here is the whole of it:")))?;
        } else {
            for line in kept {
                writeln!(out, "    {}", style::DETAIL.paint(line))?;
            }
            return Ok(());
        }
    }
    for line in printed.lines() {
        writeln!(out, "    {}", style::DETAIL.paint(line))?;
    }
    Ok(())
}

fn paint_by_marks(line: &TruthLine) -> String {
    line.cut_into_stretches()
        .iter()
        .map(|(covering, stretch)| match covering {
            Covering::Comment => style::COMMENT.paint(stretch).to_string(),
            Covering::String => style::STRING.paint(stretch).to_string(),
            Covering::Tag => style::REGION.paint(stretch).to_string(),
            Covering::Residue => stretch.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn a_fragment_naming_one_case_runs_it_and_one_naming_several_lists_them_instead() {
        let (corpus, _, _) = read_everything();
        assert!(find_case(&corpus, "8040-doc_comment_with_no_text").is_ok());
        let unique = find_case(&corpus, "2090").unwrap_or_else(|refused| panic!("{refused}"));
        assert_eq!(unique.name, "2090-docstring_holding_a_comment_symbol");
        let refuse = |name: &str| {
            find_case(&corpus, name).err().unwrap_or_else(|| panic!("{name} found a case"))
        };
        let several = refuse("doc_comment_with_no_text");
        assert!(several.contains("more than one contains it"), "{several}");
        assert!(several.contains("8040-doc_comment_with_no_text"), "{several}");
        assert!(several.contains("8050-doc_comment_with_no_text_ending_a_block"), "{several}");
        assert_eq!(refuse("a_case_of_no_such_kind"), "no case is named a_case_of_no_such_kind");
    }

    #[test]
    fn every_line_comes_out_with_its_spans_its_rule_and_its_region_and_no_binary_stops_nothing() {
        colored::control::set_override(false);
        let (corpus, dialects, adapters) = read_everything();
        let case = find_case(&corpus, "8040-doc_comment_with_no_text").unwrap();
        let tokei = adapters.iter().find(|a| a.name_of_counter == "tokei").unwrap();
        let mut written = Vec::new();
        explain_one_counter(&mut written, tokei, None, case, &dialects, &corpus.readings).unwrap();
        let text = String::from_utf8(written).unwrap();
        assert!(text.contains("tokei.default on 8040-doc_comment_with_no_text"), "{text}");
        assert!(text.contains("by a-comment-alone-is-comments"), "{text}");
        assert!(text.contains("(in-comment, word-in-comment)"), "{text}");
        assert!(text.contains("in Markdown"), "{text}");
        assert!(text.contains("tokei declares no per-line command of its own"), "{text}");
    }

    #[test]
    fn a_counter_with_a_per_line_command_and_no_binary_says_so_instead_of_failing() {
        colored::control::set_override(false);
        let (corpus, dialects, adapters) = read_everything();
        let case = find_case(&corpus, "8040-doc_comment_with_no_text").unwrap();
        let scc = adapters.iter().find(|a| a.name_of_counter == "scc").unwrap();
        let mut written = Vec::new();
        explain_one_counter(&mut written, scc, None, case, &dialects, &corpus.readings).unwrap();
        let text = String::from_utf8(written).unwrap();
        assert!(text.contains("no binary named for scc, so neither what it answers nor how it \
                               reads the lines was measured"), "{text}");
    }

    #[test]
    fn what_the_counter_answers_is_held_against_its_rules_and_marked() {
        colored::control::set_override(false);
        let (corpus, dialects, _) = read_everything();
        let case = find_case(&corpus, "5060-code_then_a_spliced_line_comment").unwrap();
        let real = a_derivation(&corpus, &dialects, case);

        let agrees = ItsAnswer::Counted(real.clone());
        let text = a_header(case, &real, &agrees, &TheirAnswer::NoCommand, &[]);
        assert!(text.contains("mezura answers"), "{text}");
        assert!(text.contains("✓ agrees"), "{text}");

        let mut moved = real.clone();
        moved.counts.lines += 1;
        let text = a_header(case, &real, &ItsAnswer::Counted(moved), &TheirAnswer::NoCommand, &[]);
        assert!(text.contains("✗ differs"), "{text}");

        let text = a_header(case, &real, &ItsAnswer::Unclaimed, &TheirAnswer::NoCommand, &[]);
        assert!(text.contains("nothing, it claims no such file"), "{text}");
        assert!(!text.contains("✗"), "not claiming a file is not a failure\n{text}");

        let broke = ItsAnswer::Broke("exit status 101".to_string());
        let text = a_header(case, &real, &broke, &TheirAnswer::NoCommand, &[]);
        assert!(text.contains("mezura could not be run: exit status 101"), "{text}");
    }

    #[test]
    fn an_answer_whose_regions_alone_differ_is_marked_and_says_which_ones() {
        colored::control::set_override(false);
        let (corpus, dialects, _) = read_everything();
        let case = find_case(&corpus, "6090-vue_blocks_count_as_their_own_languages").unwrap();
        let real = a_derivation(&corpus, &dialects, case);
        assert!(!real.regions.is_empty(), "the case is the one with regions in it");

        let mut lost = real.clone();
        lost.regions.remove(0);
        let text = a_header(case, &real, &ItsAnswer::Counted(lost), &TheirAnswer::NoCommand, &[]);
        assert!(text.contains("✗ differs"), "{text}");
        assert!(text.contains("in its regions"), "{text}");
    }

    #[test]
    fn a_line_the_counter_reads_differently_is_named_under_that_line_and_counted_at_the_top() {
        colored::control::set_override(false);
        let (corpus, dialects, _) = read_everything();
        let case = find_case(&corpus, "5060-code_then_a_spliced_line_comment").unwrap();
        let dialect = dialects.find("mezura", "content").unwrap();
        let explained = explain_every_line(&case.truth, dialect, &corpus.readings).unwrap();
        let real = a_derivation(&corpus, &dialects, case);

        let block = a_block(case);
        let agreeing = TheirAnswer::PerLine(a_counter_saying(&["code", "comments", "code"], &real));
        let mut written = Vec::new();
        write_the_header(&mut written, &block, &real, &ItsAnswer::NotMeasured, &agreeing, &explained)
            .unwrap();
        write_every_line(&mut written, &block, &explained, &agreeing).unwrap();
        let text = String::from_utf8(written).unwrap();
        assert!(text.contains("mezura reads every line the same way"), "{text}");
        assert!(!text.contains("mezura says"), "{text}");

        let differing = TheirAnswer::PerLine(a_counter_saying(&["code", "code", "code"], &real));
        let mut written = Vec::new();
        write_the_header(&mut written, &block, &real, &ItsAnswer::NotMeasured, &differing, &explained)
            .unwrap();
        write_every_line(&mut written, &block, &explained, &differing).unwrap();
        let text = String::from_utf8(written).unwrap();
        assert!(text.contains("mezura reads 1 line differently"), "{text}");
        assert!(text.contains("mezura says code"), "{text}");
    }

    #[test]
    fn a_counter_that_prints_a_document_nobody_can_read_says_what_is_wrong_with_it() {
        colored::control::set_override(false);
        let (corpus, dialects, _) = read_everything();
        let case = find_case(&corpus, "5060-code_then_a_spliced_line_comment").unwrap();
        let real = a_derivation(&corpus, &dialects, case);
        let unreadable = TheirAnswer::Unreadable("it answers 4 lines, and the file has 3".to_string());
        let text = a_header(case, &real, &ItsAnswer::NotMeasured, &unreadable, &[]);
        assert!(text.contains("what mezura printed could not be read: it answers 4 lines"), "{text}");
    }

    #[test]
    fn a_painted_line_keeps_every_byte_of_the_source() {
        colored::control::set_override(false);
        let source = "a = \"one\"; // two";
        let marked = |marker: &str| TruthLine {
            source: source.to_string(),
            marker: marker.to_string(),
            regions: Vec::new(),
        };
        assert_eq!(source, paint_by_marks(&marked("... SsssZ. CCcccc")));
        assert_eq!(source, paint_by_marks(&marked("")));
    }

    #[test]
    fn only_the_lines_holding_the_declared_text_are_kept_and_from_that_text_on() {
        colored::control::set_override(false);
        let printed = "TRACE 12:00: a/b.py line 1 ended: counted as code\n\
                       TRACE 12:00: nanoseconds process: 0\n\
                       Language,Lines\nPython,5\n";
        let mut written = Vec::new();
        write_what_it_printed(&mut written, printed, Some("line ")).unwrap();
        assert_eq!(String::from_utf8(written).unwrap(), "    line 1 ended: counted as code\n");

        let mut nothing_matches = Vec::new();
        write_what_it_printed(&mut nothing_matches, printed, Some("counted lines:")).unwrap();
        let text = String::from_utf8(nothing_matches).unwrap();
        assert!(text.contains("nothing it printed holds [counted lines:]"), "{text}");
        assert!(text.contains("    TRACE 12:00: nanoseconds process: 0"), "{text}");

        let mut verbatim = Vec::new();
        write_what_it_printed(&mut verbatim, printed, None).unwrap();
        assert!(String::from_utf8(verbatim).unwrap().starts_with("    TRACE 12:00: a/b.py"));
    }

    fn a_header(
        case: &Case,
        real: &Answer,
        its: &ItsAnswer,
        theirs: &TheirAnswer,
        explained: &[ExplainedLine],
    ) -> String {
        let mut written = Vec::new();
        write_the_header(&mut written, &a_block(case), real, its, theirs, explained).unwrap();
        String::from_utf8(written).unwrap()
    }

    fn a_block(case: &Case) -> OneWay<'_> {
        OneWay { counter: "mezura", way: "content", case }
    }

    fn a_derivation(corpus: &Corpus, dialects: &Dialects, case: &Case) -> Answer {
        let dialect = dialects.find("mezura", "content").unwrap();
        derive_answer(&case.truth, dialect, &corpus.readings).unwrap().real
    }

    fn a_counter_saying(buckets_of_lines: &[&str], real: &Answer) -> PerLineAnswer {
        let mut counts = Counts { lines: 0, buckets: real.counts.buckets.clone() };
        for value in counts.buckets.values_mut() {
            *value = 0;
        }
        for bucket in buckets_of_lines {
            counts.lines += 1;
            *counts.buckets.get_mut(*bucket).unwrap() += 1;
        }
        PerLineAnswer {
            buckets_of_lines: buckets_of_lines.iter().map(|name| name.to_string()).collect(),
            counts,
        }
    }

    fn read_everything() -> (Corpus, Dialects, Vec<Adapter>) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let dialects = Dialects::read(&[root.join("dialects")])
            .unwrap_or_else(|faults| panic!("{}", faults.join("\n")));
        let corpus = Corpus::read(&root.join("cases"))
            .unwrap_or_else(|faults| panic!("{faults:?}"));
        let adapters = Adapter::read_all(&[root.join("adapters")], &dialects).unwrap();
        (corpus, dialects, adapters)
    }
}
