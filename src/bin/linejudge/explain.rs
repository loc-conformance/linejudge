use std::io::{self, Write};
use std::path::Path;

use linejudge::adapter::Adapter;
use linejudge::answer::Counts;
use linejudge::corpus::{Case, Corpus};
use linejudge::deriver::{ExplainedLine, explain_every_line};
use linejudge::dialects::{Dialect, Dialects};
use linejudge::readings::Readings;

use crate::report::format_counts;

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
/// rule that took it and the predicates that hold on it, and under those whatever the counter
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
                    writeln!(out, "  {fault}")?;
                }
                continue;
            }
        };
        writeln!(out, "\n{key} on {}: {}", case.name, format_counts(&sum(&explained, dialect)))?;

        let width = case.truth.lines.len().to_string().len();
        for (index, (line, truth_line)) in explained.iter().zip(&case.truth.lines).enumerate() {
            writeln!(out, "  {:>width$}  {}", index + 1, truth_line.source)?;
            if !truth_line.marker.trim().is_empty() {
                writeln!(out, "  {:>width$}  {}", "", truth_line.marker)?;
            }
            let mut verdict = format!("{}, by {}", line.bucket, line.rules.join(" and by "));
            if !line.holds.is_empty() {
                verdict.push_str(&format!("   ({})", line.holds.join(", ")));
            }
            if let Some(region) = &line.region {
                verdict.push_str(&format!("   in {region}"));
            }
            writeln!(out, "  {:>width$}  {}", "", verdict)?;
        }

        if adapter.explain_args.is_none() {
            writeln!(out, "  {} declares no per-line command of its own", adapter.name_of_counter)?;
            continue;
        }
        let Some(binary) = binary else {
            writeln!(
                out,
                "  {} has a per-line command and no binary named to run it",
                adapter.name_of_counter
            )?;
            continue;
        };
        if let Some(command) = adapter.format_explain_command(way, binary, &case.input_file) {
            writeln!(out, "\n  what {} itself says, from {command}:", adapter.name_of_counter)?;
        }
        match adapter.run_explain(way, binary, &case.input_file) {
            Some(Ok(printed)) => {
                write_what_it_printed(out, &printed, adapter.explain_keep_from.as_deref())?;
            }
            Some(Err(message)) => writeln!(out, "    it could not be run: {message}")?,
            None => {}
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
            writeln!(out, "    nothing it printed holds [{text}], so here is the whole of it:")?;
        } else {
            for line in kept {
                writeln!(out, "    {line}")?;
            }
            return Ok(());
        }
    }
    for line in printed.lines() {
        writeln!(out, "    {line}")?;
    }
    Ok(())
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
        let (corpus, dialects, adapters) = read_everything();
        let case = find_case(&corpus, "4900-doc_comment_with_no_text").unwrap();
        let tokei = adapters.iter().find(|a| a.name_of_counter == "tokei").unwrap();
        let mut written = Vec::new();
        explain_one_counter(&mut written, tokei, None, case, &dialects, &corpus.readings).unwrap();
        let text = String::from_utf8(written).unwrap();
        assert!(text.contains("tokei.default on 4900-doc_comment_with_no_text:"), "{text}");
        assert!(text.contains("by a-comment-alone-is-comments"), "{text}");
        assert!(text.contains("(in-comment, word-in-comment)"), "{text}");
        assert!(text.contains("in Markdown"), "{text}");
        assert!(text.contains("tokei declares no per-line command of its own"), "{text}");
    }

    #[test]
    fn a_counter_with_a_per_line_command_and_no_binary_says_so_instead_of_failing() {
        let (corpus, dialects, adapters) = read_everything();
        let case = find_case(&corpus, "4900-doc_comment_with_no_text").unwrap();
        let scc = adapters.iter().find(|a| a.name_of_counter == "scc").unwrap();
        let mut written = Vec::new();
        explain_one_counter(&mut written, scc, None, case, &dialects, &corpus.readings).unwrap();
        let text = String::from_utf8(written).unwrap();
        assert!(text.contains("scc has a per-line command and no binary named"), "{text}");
    }

    #[test]
    fn only_the_lines_holding_the_declared_text_are_kept_and_from_that_text_on() {
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

    fn read_everything() -> (Corpus, Dialects, Vec<Adapter>) {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let dialects = Dialects::read(&root.join("dialects"))
            .unwrap_or_else(|faults| panic!("{}", faults.join("\n")));
        let corpus = Corpus::read(&root.join("cases"), &dialects)
            .unwrap_or_else(|faults| panic!("{faults:?}"));
        let adapters = Adapter::read_all(&root.join("adapters"), &dialects).unwrap();
        (corpus, dialects, adapters)
    }
}
