use std::collections::BTreeMap;

use serde::Deserialize;

use crate::measurement::count_lines;
use crate::per_line::{PerLineAnswer, build_answer};

const STAGE_PREFIX: &str = "->";

// cloc prints its totals and no account of where it put each line, so its per-line answer is
// worked out from `--print-filter-stages`, which prints what is left of the file after each of
// its comment filters. Each stage numbers the surviving lines from one every time, so the numbers
// say nothing about where a line began and the survivors are aligned to the original by text.
pub fn read_per_line(
    buckets: &[String],
    lines_of_the_file: usize,
    text: &str,
) -> Result<PerLineAnswer, String> {
    let stages = collect_stages(text);
    if stages.len() < 2 {
        return Err("it holds no filter stages".to_string());
    }
    let original = &stages[0];
    let after_blanks = &stages[1];
    let survived = &stages[stages.len() - 1];

    let lined_up = find_removed_lines(original, after_blanks)
        .zip(find_removed_lines(after_blanks, survived));
    let Some((blank_at, comment_at)) = lined_up else {
        return Err("the surviving lines cannot be lined up with the file".to_string());
    };
    let mut bucket_of = vec!["code"; original.len()];
    for &at in &blank_at {
        bucket_of[at] = "blanks";
    }
    // The second alignment counts in the numbering the blank pass left behind, so it is walked
    // back to the original lines.
    let kept: Vec<usize> = (0..original.len()).filter(|&at| bucket_of[at] == "code").collect();
    for &at in &comment_at {
        bucket_of[kept[at]] = "comments";
    }

    let mut totals: BTreeMap<String, u32> = BTreeMap::new();
    for name in ["blanks", "code", "comments"] {
        totals.insert(name.to_string(), 0);
    }
    for bucket in &bucket_of {
        *totals.entry((*bucket).to_string()).or_default() += 1;
    }
    check_against_what_cloc_said(&totals, text)?;

    let verdicts = bucket_of
        .iter()
        .enumerate()
        .map(|(at, bucket)| (at as u32 + 1, (*bucket).to_string()))
        .collect();
    let lines = count_lines(original.len() as u64, "the file")?;
    build_answer(lines, totals, verdicts, buckets, lines_of_the_file)
}

// Each stage is a `->... :` header followed by one `   N | text` line per line still standing.
fn collect_stages(text: &str) -> Vec<Vec<String>> {
    let mut stages: Vec<Vec<String>> = Vec::new();
    for line in text.lines() {
        if is_stage_header(line) {
            stages.push(Vec::new());
        } else if let Some(stage) = stages.last_mut()
            && let Some(kept) = strip_line_number(line)
        {
            stage.push(kept.to_string());
        }
    }
    stages
}

fn is_stage_header(line: &str) -> bool {
    let Some(rest) = line.strip_prefix(STAGE_PREFIX) else { return false };
    let rest = rest.trim_end();
    rest.ends_with("Original file:")
        || rest.ends_with("Blank lines removed:")
        || rest.contains("After ")
        || (rest.contains("post ") && rest.ends_with("blank cleanup:"))
}

fn strip_line_number(line: &str) -> Option<&str> {
    let numbered = line.trim_start();
    let text = numbered.trim_start_matches(|c: char| c.is_ascii_digit());
    if text.len() == numbered.len() {
        return None;
    }
    text.strip_prefix(" | ")
}

// The line a survivor came from, searching forward from `at`. A line holding code and a comment
// together is not dropped, it is rewritten with the comment cut out, so what is left of it can sit
// anywhere inside the line it came from and the survivor is looked for as a substring. The trap: a
// comment-only line, which is dropped, can itself contain a later code line word for word
// ("// x = 1;" above "x = 1;"). When the nearest line to contain the survivor only contains it
// while a later line equals it outright, the survivor could have come from either, so nothing is
// returned and the file is refused rather than have its two lines' buckets guessed and possibly
// swapped.
fn find_source_of(before: &[String], at: usize, survivor: &str) -> Option<usize> {
    let wanted = survivor.trim();
    let mut nearest = None;
    for (index, line) in before.iter().enumerate().skip(at) {
        let line = line.trim();
        let holds = if wanted.is_empty() { line.is_empty() } else { line.contains(wanted) };
        if nearest.is_none() && holds {
            nearest = Some(index);
        }
        if line == wanted {
            return if nearest == Some(index) { nearest } else { None };
        }
    }
    nearest
}

// Which lines a stage dropped. Where a survivor cannot be placed, the file is one whose lines
// cloc joined together, and no line can honestly be pointed at.
fn find_removed_lines(before: &[String], after: &[String]) -> Option<Vec<usize>> {
    let mut gone = Vec::new();
    let mut at = 0;
    for line in after {
        let found = find_source_of(before, at, line)?;
        gone.extend(at..found);
        at = found + 1;
    }
    gone.extend(at..before.len());
    Some(gone)
}

// What cloc itself printed for the file, in the JSON after the stages. A reading that does not
// add up to it is not handed over.
fn check_against_what_cloc_said(totals: &BTreeMap<String, u32>, text: &str) -> Result<(), String> {
    let json_at = match text.starts_with('{') {
        true => Some(0),
        false => text.find("\n{").map(|at| at + 1),
    };
    let Some(json_at) = json_at else {
        return Err("it holds no totals after the stages".to_string());
    };
    let report: ClocReport = serde_json::from_str(&text[json_at..])
        .map_err(|e| format!("the totals cloc printed do not parse: {e}"))?;
    let counted = |name: &str| u64::from(totals.get(name).copied().unwrap_or_default());
    let said = report.sum;
    if counted("blanks") != said.blank
        || counted("comments") != said.comment
        || counted("code") != said.code
    {
        return Err(format!(
            "the lines add up to {} blank, {} comment, {} code and cloc says {}, {}, {}",
            counted("blanks"),
            counted("comments"),
            counted("code"),
            said.blank,
            said.comment,
            said.code
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
struct ClocReport {
    #[serde(rename = "SUM")]
    sum: ClocSum,
}

#[derive(Deserialize)]
struct ClocSum {
    blank: u64,
    comment: u64,
    code: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAGES: &str = include_str!("../../tests/fixtures/output/cloc-stages.txt");
    const AMBIGUOUS: &str = include_str!("../../tests/fixtures/output/cloc-stages-ambiguous.txt");
    const BUCKETS: [&str; 3] = ["code", "comments", "blanks"];

    // Git stores the fixture with LF and cloc on Windows prints CRLF, so both endings are judged
    // from the one file whatever the checkout wrote
    fn with_each_line_ending(text: &str) -> [String; 2] {
        let lf = text.replace("\r\n", "\n");
        let crlf = lf.replace('\n', "\r\n");
        [lf, crlf]
    }

    #[test]
    fn the_stages_become_one_verdict_per_original_line() {
        for stages in with_each_line_ending(STAGES) {
            let answer = read_per_line(&named(), 5, &stages).unwrap();
            assert_eq!(answer.buckets_of_lines, ["code", "blanks", "comments", "comments", "code"]);
            assert_eq!(answer.counts.lines, 5);
            assert_eq!(answer.counts.buckets["code"], 2);
            assert_eq!(answer.counts.buckets["comments"], 2);
            assert_eq!(answer.counts.buckets["blanks"], 1);
        }
    }

    // A dropped comment-only line holding a later code line word for word: saying which of the
    // two survived would be a guess, so the file is refused.
    #[test]
    fn a_survivor_two_lines_could_have_produced_refuses_the_file() {
        let refused = read_per_line(&named(), 2, AMBIGUOUS).unwrap_err();
        assert!(refused.contains("cannot be lined up"), "{refused}");
    }

    #[test]
    fn a_survivor_no_line_could_have_produced_refuses_the_file() {
        let [stages, _] = with_each_line_ending(STAGES);
        let vanished = stages.replace("    2 | int b; \n{", "    2 | vanished\n{");
        assert_ne!(vanished, stages, "the survivor of the last stage was not found to replace");
        let refused = read_per_line(&named(), 5, &vanished).unwrap_err();
        assert!(refused.contains("cannot be lined up"), "{refused}");
    }

    #[test]
    fn a_reading_that_does_not_add_up_to_what_cloc_said_is_refused() {
        let moved = STAGES.replace("\"code\": 2", "\"code\": 3");
        let refused = read_per_line(&named(), 5, &moved).unwrap_err();
        assert!(refused.contains("add up to 1 blank, 2 comment, 2 code and cloc says 1, 2, 3"), "{refused}");
    }

    #[test]
    fn output_with_no_stages_in_it_is_refused() {
        let refused = read_per_line(&named(), 5, "nothing cloc ever printed").unwrap_err();
        assert!(refused.contains("no filter stages"), "{refused}");
    }

    fn named() -> Vec<String> {
        BUCKETS.map(String::from).to_vec()
    }
}
