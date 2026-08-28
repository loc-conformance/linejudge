use std::collections::BTreeMap;

use serde::Deserialize;

use crate::measurement::count_lines;
use crate::per_line::{PerLineAnswer, build_answer};

const STATE_MARK: &str = " ended with state:";
const LINE_MARK: &str = " line ";
const COUNTED_MARK: &str = "counted as ";

// scc says nothing per line in its JSON, but with `-t` its trace prints one
// `<file> line N ended with state: S: counted as X` for every line, on stdout, beside the counts.
pub fn read_per_line(
    buckets: &[String],
    lines_of_the_file: usize,
    text: &str,
) -> Result<PerLineAnswer, String> {
    let verdicts = collect_verdicts(text)?;
    let counted = find_counts(text)?;
    let totals = BTreeMap::from([
        ("blanks".to_string(), count_lines(counted.blank, "scc's blanks")?),
        ("code".to_string(), count_lines(counted.code, "scc's code")?),
        ("comments".to_string(), count_lines(counted.comment, "scc's comments")?),
    ]);
    build_answer(count_lines(counted.lines, "the file")?, totals, verdicts, buckets, lines_of_the_file)
}

fn collect_verdicts(text: &str) -> Result<Vec<(u32, String)>, String> {
    let mut verdicts = Vec::new();
    for line in text.lines() {
        let Some(state_at) = line.rfind(STATE_MARK) else { continue };
        let before = &line[..state_at];
        let Some(number_at) = before.rfind(LINE_MARK) else { continue };
        let Ok(number) = before[number_at + LINE_MARK.len()..].trim().parse::<u32>() else {
            continue;
        };
        let after = &line[state_at + STATE_MARK.len()..];
        let Some(counted_at) = after.find(COUNTED_MARK) else { continue };
        let bucket = match after[counted_at + COUNTED_MARK.len()..].trim() {
            "code" => "code",
            "comment" => "comments",
            "blank" => "blanks",
            word => {
                return Err(format!(
                    "it counts line {number} as {word}, which is not code, comment or blank"
                ));
            }
        };
        verdicts.push((number, bucket.to_string()));
    }
    Ok(verdicts)
}

// The counts are the last line, a JSON array with one entry per language, and one file is one
// language.
fn find_counts(text: &str) -> Result<SccCounts, String> {
    let Some(printed) = text.lines().rev().find(|line| !line.trim().is_empty()) else {
        return Err("it is empty".to_string());
    };
    let mut counted: Vec<SccCounts> = serde_json::from_str(printed.trim())
        .map_err(|e| format!("its counts do not parse: {e}"))?;
    match counted.len() {
        0 => Err("it claims no such file".to_string()),
        1 => Ok(counted.remove(0)),
        several => Err(format!("it answers as {several} languages for the one file")),
    }
}

#[derive(Deserialize)]
struct SccCounts {
    #[serde(rename = "Lines")]
    lines: u64,
    #[serde(rename = "Code")]
    code: u64,
    #[serde(rename = "Comment")]
    comment: u64,
    #[serde(rename = "Blank")]
    blank: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    const TRACE: &str = include_str!("../../tests/fixtures/output/scc-trace.txt");
    const BUCKETS: [&str; 3] = ["code", "comments", "blanks"];

    #[test]
    fn the_trace_becomes_one_verdict_per_line_and_the_counts_become_the_totals() {
        let answer = read_per_line(&named(), 5, TRACE).unwrap();
        assert_eq!(answer.buckets_of_lines, ["code", "blanks", "comments", "comments", "code"]);
        assert_eq!(answer.counts.lines, 5);
        assert_eq!(answer.counts.buckets["code"], 2);
        assert_eq!(answer.counts.buckets["comments"], 2);
        assert_eq!(answer.counts.buckets["blanks"], 1);
    }

    #[test]
    fn a_line_the_trace_never_mentions_is_refused_by_the_count_of_verdicts() {
        let cut = TRACE
            .lines()
            .filter(|line| !line.contains("line 2 "))
            .collect::<Vec<_>>()
            .join("\n");
        let refused = read_per_line(&named(), 5, &cut).unwrap_err();
        assert!(refused.contains("it answers 4 lines, and the file has 5"), "{refused}");
    }

    #[test]
    fn counts_that_disagree_with_the_trace_are_refused() {
        let moved = TRACE.replace("\"Code\":2", "\"Code\":3");
        let refused = read_per_line(&named(), 5, &moved).unwrap_err();
        assert!(refused.contains("it counts 3 code for the file and 2 of its lines say code"), "{refused}");
    }

    #[test]
    fn a_file_scc_does_not_claim_is_said_so_and_a_word_this_reader_does_not_know_is_named() {
        let unclaimed = "TRACE 12:00: configured to lazy load language features\n[]\n";
        assert_eq!(read_per_line(&named(), 3, unclaimed).unwrap_err(), "it claims no such file");

        let strange = TRACE.replace("counted as blank", "counted as slack");
        let refused = read_per_line(&named(), 5, &strange).unwrap_err();
        assert!(refused.contains("it counts line 2 as slack"), "{refused}");
    }

    fn named() -> Vec<String> {
        BUCKETS.map(String::from).to_vec()
    }
}
