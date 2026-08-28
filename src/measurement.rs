use std::collections::BTreeMap;

use serde::Deserialize;

use crate::answer::{Answer, Counts, RegionCounts};
use crate::dialects::check_buckets;
use crate::readers::tokei;

const KNOWN_FORMAT: u32 = 1;

// A reader under `src/readers/`, for a counter whose output the `read` block of an adapter cannot
// describe. Anything describable declares a `read` block instead and never lands in this list.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum OutputFormat {
    #[serde(rename = "tokei-json")]
    TokeiJson,
    // Already the shape this suite compares, whatever a counter's buckets are called.
    #[serde(rename = "linejudge-json")]
    LinejudgeJson,
}

// Every count read here comes back under one of `buckets`. `None` is a counter that does not claim
// the file, which is not the same answer as zeroes.
pub fn read_output(
    output: OutputFormat,
    buckets: &[String],
    text: &str,
) -> Result<Option<Answer>, String> {
    match output {
        OutputFormat::TokeiJson => tokei::read_counts(buckets, text),
        OutputFormat::LinejudgeJson => read_linejudge(buckets, text),
    }
}

// A count too large for the file is said out loud rather than wrapped around and presented as an
// answer.
pub(crate) fn count_lines(counted: u64, whose: &str) -> Result<u32, String> {
    u32::try_from(counted).map_err(|_| format!("{whose} was counted as {counted} lines"))
}

pub(crate) fn parse<T: for<'a> Deserialize<'a>>(text: &str) -> Result<T, String> {
    serde_json::from_str(text).map_err(|e| format!("what the counter printed does not parse: {e}"))
}

pub(crate) fn sort_regions(mut regions: Vec<RegionCounts>) -> Vec<RegionCounts> {
    regions.sort();
    regions
}

// `null` is a file the counter does not claim.
fn read_linejudge(buckets: &[String], text: &str) -> Result<Option<Answer>, String> {
    let raw: Option<LinejudgeAnswer> = parse(text)?;
    let Some(raw) = raw else { return Ok(None) };
    if raw.format != KNOWN_FORMAT {
        return Err(format!(
            "it declares format {}, and the format this reads is {KNOWN_FORMAT}",
            raw.format
        ));
    }
    check_buckets(&raw.buckets, buckets)?;
    let mut regions = Vec::new();
    for region in raw.regions {
        check_buckets(&region.buckets, buckets)
            .map_err(|e| format!("in the {} region: {e}", region.language))?;
        regions.push(RegionCounts {
            language: region.language,
            lines: region.lines,
            buckets: region.buckets,
        });
    }
    Ok(Some(Answer {
        counts: Counts { lines: raw.lines, buckets: raw.buckets },
        regions: sort_regions(regions),
    }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinejudgeAnswer {
    format: u32,
    lines: u32,
    buckets: BTreeMap<String, u32>,
    #[serde(default)]
    regions: Vec<LinejudgeRegion>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinejudgeRegion {
    language: String,
    lines: u32,
    buckets: BTreeMap<String, u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const WITH_BLANKS: [&str; 3] = ["code", "comments", "blanks"];

    // Also the one test that reaches tokei's reader through the dispatch above, so a rewired
    // match arm cannot go unnoticed.
    #[test]
    fn a_counter_that_claims_nothing_is_not_a_counter_that_answered_zero() {
        let text = r#"{"Total":{"blanks":0,"code":0,"comments":0,"children":{}}}"#;
        assert!(read_output(OutputFormat::TokeiJson, &named(&WITH_BLANKS), text).unwrap().is_none());
    }

    #[test]
    fn the_uniform_format_is_read_as_printed_and_null_claims_nothing() {
        let text = r#"{"format": 1, "lines": 4, "buckets": {"code": 2, "comments": 1, "blanks": 1},
            "regions": [{"language": "CSS", "lines": 2,
                         "buckets": {"code": 1, "comments": 1, "blanks": 0}}]}"#;
        let uniform = measure(OutputFormat::LinejudgeJson, &WITH_BLANKS, text);
        assert_eq!(as_numbers(&uniform.counts, "blanks"), (4, 2, 1, 1));
        assert_eq!(as_numbers_of_region(&uniform.regions[0], "blanks"), (2, 1, 1, 0));
        let nothing = read_output(OutputFormat::LinejudgeJson, &named(&WITH_BLANKS), "null");
        assert!(nothing.unwrap().is_none());
    }

    #[test]
    fn a_fourth_bucket_the_dialect_declares_is_read_like_any_other() {
        let four = ["code", "comments", "documentation", "blanks"];
        let text = r#"{"format": 1, "lines": 4, "buckets":
            {"code": 2, "comments": 0, "documentation": 1, "blanks": 1}}"#;
        let uniform = measure(OutputFormat::LinejudgeJson, &four, text);
        assert_eq!(uniform.counts.buckets["documentation"], 1);
    }

    #[test]
    fn a_bucket_this_dialect_has_not_is_refused_by_its_name() {
        let text = r#"{"format": 1, "lines": 4, "buckets": {"code": 2, "comments": 1, "blank": 1}}"#;
        let refused =
            read_output(OutputFormat::LinejudgeJson, &named(&WITH_BLANKS), text).unwrap_err();
        assert!(refused.contains("no blanks"), "{refused}");
    }

    #[test]
    fn a_format_this_build_does_not_know_is_refused_by_its_number() {
        let text = r#"{"format": 2, "lines": 1, "buckets": {"code": 1, "comments": 0, "blanks": 0}}"#;
        let refused =
            read_output(OutputFormat::LinejudgeJson, &named(&WITH_BLANKS), text).unwrap_err();
        assert!(refused.contains("declares format 2"), "{refused}");
    }

    #[test]
    fn output_that_does_not_parse_is_an_error_and_not_an_absent_answer() {
        let broken =
            read_output(OutputFormat::LinejudgeJson, &named(&WITH_BLANKS), "not json at all");
        assert!(broken.unwrap_err().contains("does not parse"));
    }

    fn measure(output: OutputFormat, buckets: &[&str], text: &str) -> Answer {
        read_output(output, &named(buckets), text).unwrap().unwrap()
    }

    fn named(buckets: &[&str]) -> Vec<String> {
        buckets.iter().map(|bucket| bucket.to_string()).collect()
    }

    fn as_numbers(counts: &Counts, third: &str) -> (u32, u32, u32, u32) {
        (counts.lines, counts.buckets["code"], counts.buckets["comments"], counts.buckets[third])
    }

    fn as_numbers_of_region(region: &RegionCounts, third: &str) -> (u32, u32, u32, u32) {
        (region.lines, region.buckets["code"], region.buckets["comments"], region.buckets[third])
    }
}
