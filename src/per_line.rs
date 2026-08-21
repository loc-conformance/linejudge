//! A counter's own account of where it put every line of a file.

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::answer::Counts;
use crate::dialects::check_buckets;

const KNOWN_FORMAT: u32 = 1;

/// The shape a counter prints that account in. There is one, and it is the same for every counter:
/// a tool that prints it needs no reader written for it here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum PerLineFormat {
    /// The JSON document this module reads, declared in an adapter as `linejudge-per-line`.
    #[serde(rename = "linejudge-per-line")]
    LinejudgePerLine,
}

/// What one counter says about one file, line by line.
#[derive(Debug, PartialEq, Eq)]
pub struct PerLineAnswer {
    /// The bucket it put each physical line in, in order from the first.
    pub buckets_of_lines: Vec<String>,
    /// The totals it printed for the file as a whole.
    pub counts: Counts,
}

/// Reads such a document, given the buckets this way of counting has and how many lines the file
/// really holds. A counter that answers a seventeen line file with eighteen verdicts is told what
/// is wrong with its document instead of being compared against the wrong lines.
pub fn read_per_line(
    text: &str,
    buckets: &[String],
    lines_of_the_file: usize,
) -> Result<PerLineAnswer, String> {
    let raw: RawDocument =
        serde_json::from_str(text).map_err(|e| format!("it does not parse: {e}"))?;
    if raw.format != KNOWN_FORMAT {
        return Err(format!(
            "it declares format {}, and the format this reads is {KNOWN_FORMAT}",
            raw.format
        ));
    }
    check_buckets(&raw.buckets, buckets).map_err(|e| format!("it {e}"))?;
    if raw.per_line.len() != lines_of_the_file {
        return Err(format!(
            "it answers {} lines, and the file has {lines_of_the_file}",
            raw.per_line.len()
        ));
    }
    if raw.lines as usize != lines_of_the_file {
        return Err(format!(
            "it says the file has {} lines, and it has {lines_of_the_file}",
            raw.lines
        ));
    }

    let mut buckets_of_lines = Vec::with_capacity(raw.per_line.len());
    let mut summed: BTreeMap<&str, u32> = BTreeMap::new();
    for (index, verdict) in raw.per_line.iter().enumerate() {
        let expected = index as u32 + 1;
        if verdict.line != expected {
            return Err(format!(
                "its answer number {expected} is for line {}, and the format numbers them from 1 \
                 with none missing",
                verdict.line
            ));
        }
        if !buckets.contains(&verdict.bucket) {
            return Err(format!(
                "it puts line {} in {}, which is not one of its buckets: {}",
                verdict.line,
                verdict.bucket,
                buckets.join(", ")
            ));
        }
        *summed.entry(verdict.bucket.as_str()).or_default() += 1;
        buckets_of_lines.push(verdict.bucket.clone());
    }
    for (name, total) in &raw.buckets {
        let counted = summed.get(name.as_str()).copied().unwrap_or_default();
        if counted != *total {
            return Err(format!(
                "it counts {total} {name} for the file and {counted} of its lines say {name}"
            ));
        }
    }

    Ok(PerLineAnswer {
        buckets_of_lines,
        counts: Counts { lines: raw.lines, buckets: raw.buckets },
    })
}

// No `deny_unknown_fields`, deliberately: whatever a counter says beyond what the format promises
// is its own business and rides along unread.
#[derive(Deserialize)]
struct RawDocument {
    format: u32,
    lines: u32,
    buckets: BTreeMap<String, u32>,
    per_line: Vec<RawVerdict>,
}

#[derive(Deserialize)]
struct RawVerdict {
    line: u32,
    bucket: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_document_comes_back_as_one_bucket_per_line_and_the_totals_it_printed() {
        let read = read_per_line(&a_document(), &buckets(), 3).unwrap();
        assert_eq!(read.buckets_of_lines, ["code", "comments", "code"]);
        assert_eq!(read.counts.lines, 3);
        assert_eq!(read.counts.buckets["comments"], 1);
    }

    #[test]
    fn the_fields_a_reader_does_not_know_are_carried_past_it() {
        let with_extras = a_document().replace(
            "{ \"line\": 2, \"bucket\": \"comments\" }",
            "{ \"line\": 2, \"bucket\": \"comments\", \"class\": \"words_in_comment\", \
             \"spans\": [[0, 4, \"comment\"]] }",
        );
        assert_eq!(read_per_line(&with_extras, &buckets(), 3).unwrap().buckets_of_lines[1], "comments");
    }

    #[test]
    fn a_verdict_for_every_line_is_the_promise_and_a_document_that_breaks_it_is_named() {
        let refused = |text: &str, lines: usize| {
            read_per_line(text, &buckets(), lines).err().unwrap_or_else(|| panic!("{text} was read"))
        };
        assert!(refused(&a_document(), 4).contains("it answers 3 lines, and the file has 4"));
        assert!(refused("nothing json about it", 3).contains("does not parse"));
        assert!(refused(&a_document().replace("\"format\": 1", "\"format\": 2"), 3)
                .contains("it declares format 2"));

        let renumbered = a_document().replace("\"line\": 2", "\"line\": 4");
        assert!(refused(&renumbered, 3).contains("its answer number 2 is for line 4"));
        let miscounted = a_document().replace("\"lines\": 3", "\"lines\": 7");
        assert!(refused(&miscounted, 3).contains("it says the file has 7 lines"));
    }

    #[test]
    fn totals_that_do_not_add_up_to_the_lines_under_them_are_refused() {
        let text = a_document().replace("\"code\": 2", "\"code\": 3");
        let refused = read_per_line(&text, &buckets(), 3).unwrap_err();
        assert!(refused.contains("it counts 3 code for the file and 2 of its lines say code"), "{refused}");
    }

    #[test]
    fn a_bucket_this_way_of_counting_does_not_have_is_refused_wherever_it_appears() {
        let renamed = a_document().replace("\"bucket\": \"comments\"", "\"bucket\": \"documentation\"");
        let refused = read_per_line(&renamed, &buckets(), 3).unwrap_err();
        assert!(refused.contains("it puts line 2 in documentation"), "{refused}");

        let extra_total = a_document().replace("\"extra\": 0", "\"extra\": 0, \"docs\": 0");
        let refused = read_per_line(&extra_total, &buckets(), 3).unwrap_err();
        assert!(refused.contains("has a bucket named docs"), "{refused}");
    }

    fn a_document() -> String {
        "{\n  \"format\": 1,\n  \"counter\": \"mezura\",\n  \"lines\": 3,\n  \
         \"buckets\": { \"code\": 2, \"comments\": 1, \"extra\": 0 },\n  \"per_line\": [\n    \
         { \"line\": 1, \"bucket\": \"code\" },\n    \
         { \"line\": 2, \"bucket\": \"comments\" },\n    \
         { \"line\": 3, \"bucket\": \"code\" }\n  ]\n}"
            .to_string()
    }

    fn buckets() -> Vec<String> {
        ["code", "comments", "extra"].map(String::from).to_vec()
    }
}
