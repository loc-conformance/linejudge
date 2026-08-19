use std::io::{self, Write};
use std::path::Path;

use linejudge::adapter::{Adapter, Dialect as Way};
use linejudge::answer::Counts;
use linejudge::corpus::{Case, Corpus};
use linejudge::deriver::{ExplainedLine, explain_every_line};
use linejudge::dialects::{Dialect, Dialects};
use linejudge::per_line::{PerLineAnswer, PerLineFormat, read_per_line};
use linejudge::readings::Readings;
use linejudge::truth::{COMMENT_MARKS, RESIDUE, STRING_MARKS, TAG_CLOSES, TAG_OPENS};

use crate::report::paint_counts;
use crate::style;

const BY_ITS_RULES: &str = "by its rules";

/// A whole name wins outright, and a fragment is enough where it names exactly one case; what a
/// fragment matching several gets is their list, never a guess among them.
pub fn find_case<'c>(corpus: &'c Corpus, name: &str) -> Result<&'c Case, String> {
    if let Some(case) = corpus.cases.iter().find(|case| case.name == name) {
        return Ok(case);
    }
    let close: Vec<&Case> =
        corpus.cases.iter().filter(|case| case.name.contains(name)).collect();
    match close.as_slice() {
        [] => Err(format!("no case is named {name}")),
        [one] => Ok(one),
        several => {
            let named: Vec<&str> = several.iter().map(|case| case.name.as_str()).collect();
            Err(format!(
                "no case is named {name}, and more than one contains it:\n  {}",
                named.join("\n  ")
            ))
        }
    }
}

/// One block per way this counter counts: every line of the case beside its marked spans, the
/// rule that took it and the predicates that hold on it, and beside those whatever the counter
/// itself can say about the file line by line.
pub fn explain_one_counter(
    out: &mut dyn Write,
    adapter: &Adapter,
    binary: Option<&Path>,
    case: &Case,
    dialects: &Dialects,
    readings: &Readings,
) -> io::Result<()> {
    for way in &adapter.dialects {
        let key = format!("{}.{}", adapter.name_of_counter, way.name);
        let Some(dialect) = dialects.find(&adapter.name_of_counter, &way.name) else {
            writeln!(out, "\n{key} on {}: no dialect file, nothing to derive with", case.name)?;
            continue;
        };
        let explained = match explain_every_line(&case.truth, dialect, readings) {
            Ok(explained) => explained,
            Err(faults) => {
                writeln!(out, "\n{key} on {}:", case.name)?;
                for fault in faults {
                    writeln!(out, "  {}", style::DIFFERS.paint(&fault))?;
                }
                continue;
            }
        };
        let theirs = read_what_the_counter_says(adapter, way, binary, case);
        let ours = sum(&explained, dialect);
        write_the_header(out, &key, case, &adapter.name_of_counter, &ours, &theirs, &explained)?;
        write_every_line(out, case, &adapter.name_of_counter, &explained, &theirs)?;
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

/// What the counter itself had to say about the file, which is nothing at all for one that
/// declares no per-line command, and text for a person where it declares no format to read.
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

fn read_what_the_counter_says(
    adapter: &Adapter,
    way: &Way,
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

fn write_the_header(
    out: &mut dyn Write,
    key: &str,
    case: &Case,
    counter: &str,
    ours: &Counts,
    theirs: &TheirAnswer,
    explained: &[ExplainedLine],
) -> io::Result<()> {
    let answers = format!("{counter} answers");
    let width = answers.chars().count().max(BY_ITS_RULES.chars().count());
    writeln!(out, "\n{} {} {}", style::HEADING.paint(key), style::DETAIL.paint("on"),
            style::HEADING.paint(&case.name))?;
    writeln!(out, "  {}  {}", style::LABEL.paint(&format!("{BY_ITS_RULES:<width$}")),
            paint_counts(ours, None))?;

    match theirs {
        TheirAnswer::NoCommand => writeln!(out, "  {}", style::DETAIL.paint(
                &format!("{counter} declares no per-line command of its own"))),
        TheirAnswer::NoBinary => writeln!(out, "  {}", style::DETAIL.paint(&format!(
                "{counter} has a per-line command and no binary named to run it"))),
        TheirAnswer::Broken(message) => writeln!(out, "  {} {}",
                style::DIFFERS.paint(&format!("{counter} could not be run:")),
                style::DETAIL.paint(message)),
        TheirAnswer::Unreadable(message) => writeln!(out, "  {} {}",
                style::DIFFERS.paint(&format!("what {counter} printed could not be read:")),
                style::DETAIL.paint(message)),
        TheirAnswer::Text(_) => Ok(()),
        TheirAnswer::PerLine(answer) => {
            let differing = theirs.count_lines_read_differently(explained);
            let how = match differing {
                0 => "and reads every line the same way".to_string(),
                1 => style::DIFFERS.paint("and reads 1 line differently").to_string(),
                many => style::DIFFERS.paint(&format!("and reads {many} lines differently")).to_string(),
            };
            writeln!(out, "  {}  {}, {how}", style::LABEL.paint(&format!("{answers:<width$}")),
                    paint_counts(&answer.counts, Some(ours)))
        }
    }
}

/// Every line of the case: the source painted by the spans marked under it, those markers, the
/// bucket the rules put the line in, and where the counter reads that line differently, what it
/// says instead.
fn write_every_line(
    out: &mut dyn Write,
    case: &Case,
    counter: &str,
    explained: &[ExplainedLine],
    theirs: &TheirAnswer,
) -> io::Result<()> {
    let width = case.truth.lines.len().to_string().len();
    for (at, (line, truth_line)) in explained.iter().zip(&case.truth.lines).enumerate() {
        writeln!(out)?;
        let source = paint_by_marks(&truth_line.source, &truth_line.marker);
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

/// Shows the lines holding the declared text, each from that text to its end; the lines are
/// chosen and cut, never read. Where nothing holds it, everything is shown and this says so,
/// so a tool that changed what it prints falls back to the whole of it instead of to silence.
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

/// The source line painted by what every column of it is marked as. It is walked by character
/// rather than by byte, which it may be because a case input is ASCII.
fn paint_by_marks(text: &str, marker: &str) -> String {
    let marks: Vec<char> = marker.chars().collect();
    let mut painted = String::with_capacity(text.len() + 32);
    let mut stretch = String::new();
    let mut ink = Ink::Unmarked;
    for (at, ch) in text.chars().enumerate() {
        let here = Ink::of(marks.get(at).copied().unwrap_or(RESIDUE));
        if here != ink && !stretch.is_empty() {
            painted.push_str(&ink.paint(&stretch));
            stretch.clear();
        }
        ink = here;
        stretch.push(ch);
    }
    if !stretch.is_empty() {
        painted.push_str(&ink.paint(&stretch));
    }
    painted
}

#[derive(Clone, Copy, PartialEq)]
enum Ink {
    Comment,
    String,
    Tag,
    Unmarked,
}

impl Ink {
    fn of(mark: char) -> Ink {
        match mark {
            _ if STRING_MARKS.owns(mark) => Ink::String,
            _ if COMMENT_MARKS.owns(mark) => Ink::Comment,
            TAG_OPENS | TAG_CLOSES => Ink::Tag,
            _ => Ink::Unmarked,
        }
    }

    fn paint(self, text: &str) -> String {
        match self {
            Ink::Comment => style::COMMENT.paint(text).to_string(),
            Ink::String => style::STRING.paint(text).to_string(),
            Ink::Tag => style::REGION.paint(text).to_string(),
            Ink::Unmarked => text.to_string(),
        }
    }
}

fn sum(explained: &[ExplainedLine], dialect: &Dialect) -> Counts {
    let mut counts = Counts {
        lines: 0,
        buckets: dialect.buckets.iter().map(|bucket| (bucket.clone(), 0)).collect(),
    };
    for line in explained {
        counts.lines += 1;
        *counts.buckets.entry(line.bucket.clone()).or_default() += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn a_fragment_naming_one_case_runs_it_and_one_naming_several_lists_them_instead() {
        let (corpus, _, _) = read_everything();
        assert!(find_case(&corpus, "4900-doc_comment_with_no_text").is_ok());
        let unique = find_case(&corpus, "2500").unwrap_or_else(|refused| panic!("{refused}"));
        assert_eq!(unique.name, "2500-docstring_holding_a_comment_symbol");
        let refuse = |name: &str| {
            find_case(&corpus, name).err().unwrap_or_else(|| panic!("{name} found a case"))
        };
        let several = refuse("doc_comment_with_no_text");
        assert!(several.contains("more than one contains it"), "{several}");
        assert!(several.contains("4900-doc_comment_with_no_text"), "{several}");
        assert!(several.contains("5000-doc_comment_with_no_text_ending_a_block"), "{several}");
        assert_eq!(refuse("a_case_of_no_such_kind"), "no case is named a_case_of_no_such_kind");
    }

    #[test]
    fn every_line_comes_out_with_its_spans_its_rule_and_its_region_and_no_binary_stops_nothing() {
        colored::control::set_override(false);
        let (corpus, dialects, adapters) = read_everything();
        let case = find_case(&corpus, "4900-doc_comment_with_no_text").unwrap();
        let tokei = adapters.iter().find(|a| a.name_of_counter == "tokei").unwrap();
        let mut written = Vec::new();
        explain_one_counter(&mut written, tokei, None, case, &dialects, &corpus.readings).unwrap();
        let text = String::from_utf8(written).unwrap();
        assert!(text.contains("tokei.default on 4900-doc_comment_with_no_text"), "{text}");
        assert!(text.contains("by a-comment-alone-is-comments"), "{text}");
        assert!(text.contains("(in-comment, word-in-comment)"), "{text}");
        assert!(text.contains("in Markdown"), "{text}");
        assert!(text.contains("tokei declares no per-line command of its own"), "{text}");
    }

    #[test]
    fn a_counter_with_a_per_line_command_and_no_binary_says_so_instead_of_failing() {
        colored::control::set_override(false);
        let (corpus, dialects, adapters) = read_everything();
        let case = find_case(&corpus, "4900-doc_comment_with_no_text").unwrap();
        let scc = adapters.iter().find(|a| a.name_of_counter == "scc").unwrap();
        let mut written = Vec::new();
        explain_one_counter(&mut written, scc, None, case, &dialects, &corpus.readings).unwrap();
        let text = String::from_utf8(written).unwrap();
        assert!(text.contains("scc has a per-line command and no binary named"), "{text}");
    }

    #[test]
    fn a_line_the_counter_reads_differently_is_named_under_that_line_and_counted_at_the_top() {
        colored::control::set_override(false);
        let (corpus, dialects, _) = read_everything();
        let case = find_case(&corpus, "2270-code_then_a_spliced_line_comment").unwrap();
        let dialect = dialects.find("mezura", "content").unwrap();
        let explained = explain_every_line(&case.truth, dialect, &corpus.readings).unwrap();
        let ours = sum(&explained, dialect);

        let agreeing = TheirAnswer::PerLine(a_counter_saying(&["code", "comments", "code"], &ours));
        let mut written = Vec::new();
        write_the_header(&mut written, "mezura.content", case, "mezura", &ours, &agreeing, &explained).unwrap();
        write_every_line(&mut written, case, "mezura", &explained, &agreeing).unwrap();
        let text = String::from_utf8(written).unwrap();
        assert!(text.contains("reads every line the same way"), "{text}");
        assert!(!text.contains("mezura says"), "{text}");

        let differing = TheirAnswer::PerLine(a_counter_saying(&["code", "code", "code"], &ours));
        let mut written = Vec::new();
        write_the_header(&mut written, "mezura.content", case, "mezura", &ours, &differing, &explained).unwrap();
        write_every_line(&mut written, case, "mezura", &explained, &differing).unwrap();
        let text = String::from_utf8(written).unwrap();
        assert!(text.contains("reads 1 line differently"), "{text}");
        assert!(text.contains("mezura says code"), "{text}");
    }

    #[test]
    fn a_counter_that_prints_a_document_nobody_can_read_says_what_is_wrong_with_it() {
        colored::control::set_override(false);
        let (corpus, dialects, adapters) = read_everything();
        let case = find_case(&corpus, "2270-code_then_a_spliced_line_comment").unwrap();
        let mezura = adapters.iter().find(|a| a.name_of_counter == "mezura").unwrap();
        let dialect = dialects.find("mezura", "content").unwrap();
        let explained = explain_every_line(&case.truth, dialect, &corpus.readings).unwrap();
        let ours = sum(&explained, dialect);
        let unreadable = TheirAnswer::Unreadable("it answers 4 lines, and the file has 3".to_string());
        let mut written = Vec::new();
        write_the_header(&mut written, "mezura.content", case, "mezura", &ours, &unreadable, &explained).unwrap();
        let text = String::from_utf8(written).unwrap();
        assert!(text.contains("what mezura printed could not be read: it answers 4 lines"), "{text}");
        assert!(mezura.explain_args.is_some());
    }

    #[test]
    fn a_painted_line_keeps_every_byte_of_the_source() {
        colored::control::set_override(false);
        let source = "a = \"one\"; // two";
        assert_eq!(source, paint_by_marks(source, "... SsssZ. CCcccc"));
        assert_eq!(source, paint_by_marks(source, ""));
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

    fn a_counter_saying(buckets_of_lines: &[&str], ours: &Counts) -> PerLineAnswer {
        let mut counts = Counts { lines: 0, buckets: ours.buckets.clone() };
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
        let dialects = Dialects::read(&root.join("dialects"))
            .unwrap_or_else(|faults| panic!("{}", faults.join("\n")));
        let corpus = Corpus::read(&root.join("cases"))
            .unwrap_or_else(|faults| panic!("{faults:?}"));
        let adapters = Adapter::read_all(&root.join("adapters"), &dialects).unwrap();
        (corpus, dialects, adapters)
    }
}
