use std::collections::BTreeMap;

use serde::Deserialize;

use crate::answer::{Answer, Counts, RegionCounts};
use crate::dialects::check_buckets;

const TOKEI_TOTAL: &str = "Total";

// A reader written here, for a counter whose output the `read` block of an adapter cannot
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
        OutputFormat::TokeiJson => read_tokei(buckets, text),
        OutputFormat::LinejudgeJson => read_linejudge(buckets, text),
    }
}

// A count too large for the file is said out loud rather than wrapped around and presented as an
// answer.
pub(crate) fn count_lines(counted: u64, whose: &str) -> Result<u32, String> {
    u32::try_from(counted).map_err(|_| format!("{whose} was counted as {counted} lines"))
}

// `null` is a file the counter does not claim.
fn read_linejudge(buckets: &[String], text: &str) -> Result<Option<Answer>, String> {
    let raw: Option<LinejudgeAnswer> = parse(text)?;
    let Some(raw) = raw else { return Ok(None) };
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

fn read_tokei(buckets: &[String], text: &str) -> Result<Option<Answer>, String> {
    let mut languages: BTreeMap<String, TokeiLanguage> = parse(text)?;
    let Some(total) = languages.remove(TOKEI_TOTAL) else {
        return Err(format!("no {TOKEI_TOTAL} in what tokei printed"));
    };
    if languages.is_empty() {
        return Ok(None);
    }
    let mut summed: BTreeMap<String, (u64, u64, u64)> = BTreeMap::new();
    for language in languages.values() {
        for (name, reports) in &language.children {
            for report in reports {
                sum_nested_languages(name, &report.stats, &mut summed);
            }
        }
    }
    let mut regions = Vec::new();
    for (name, (code, comments, blanks)) in summed {
        let printed = BTreeMap::from([("code", code), ("comments", comments), ("blanks", blanks)]);
        regions.push(RegionCounts {
            lines: count_lines(code + comments + blanks, &name)?,
            buckets: read_buckets(&printed, buckets, "tokei")?,
            language: name,
        });
    }
    let printed = BTreeMap::from([
        ("code", u64::from(total.code)),
        ("comments", u64::from(total.comments)),
        ("blanks", u64::from(total.blanks)),
    ]);
    let whole = u64::from(total.code) + u64::from(total.comments) + u64::from(total.blanks);
    Ok(Some(Answer {
        counts: Counts {
            lines: count_lines(whole, TOKEI_TOTAL)?,
            buckets: read_buckets(&printed, buckets, "tokei")?,
        },
        regions: sort_regions(regions),
    }))
}

fn read_buckets(
    printed: &BTreeMap<&str, u64>,
    wanted: &[String],
    counter: &str,
) -> Result<BTreeMap<String, u32>, String> {
    let mut counts = BTreeMap::new();
    for bucket in wanted {
        let Some(number) = printed.get(bucket.as_str()) else {
            let named: Vec<&str> = printed.keys().copied().collect();
            return Err(format!(
                "{counter} printed no {bucket} for this file, it printed {}",
                named.join(", ")
            ));
        };
        let whose = format!("{counter}'s {bucket}");
        counts.insert(bucket.clone(), count_lines(*number, &whose)?);
    }
    Ok(counts)
}

fn parse<T: for<'a> Deserialize<'a>>(text: &str) -> Result<T, String> {
    serde_json::from_str(text).map_err(|e| format!("what the counter printed does not parse: {e}"))
}

fn sort_regions(mut regions: Vec<RegionCounts>) -> Vec<RegionCounts> {
    regions.sort();
    regions
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LinejudgeAnswer {
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

#[derive(Deserialize)]
struct TokeiLanguage {
    blanks: u32,
    code: u32,
    comments: u32,
    #[serde(default)]
    children: BTreeMap<String, Vec<TokeiReport>>,
}

#[derive(Deserialize)]
struct TokeiReport {
    stats: TokeiStats,
}

fn sum_nested_languages(
    name: &str,
    stats: &TokeiStats,
    summed: &mut BTreeMap<String, (u64, u64, u64)>,
) {
    let entry = summed.entry(name.to_string()).or_default();
    entry.0 += u64::from(stats.code);
    entry.1 += u64::from(stats.comments);
    entry.2 += u64::from(stats.blanks);
    for (child, deeper) in &stats.blobs {
        sum_nested_languages(child, deeper, summed);
    }
}

// The blobs are how tokei nests: a report's stats carry the deeper languages, each excluding its
// own children, so a page's script inside a fence sits two levels down and is summed from there.
#[derive(Deserialize)]
struct TokeiStats {
    blanks: u32,
    code: u32,
    comments: u32,
    #[serde(default)]
    blobs: BTreeMap<String, TokeiStats>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEI: &str = include_str!("../tests/fixtures/output/tokei-nested.json");
    const TOKEI_DEEP: &str = include_str!("../tests/fixtures/output/tokei-three-levels.json");
    const WITH_BLANKS: [&str; 3] = ["code", "comments", "blanks"];

    #[test]
    fn tokei_is_read_with_its_lines_as_the_three_buckets_added_up() {
        let tokei = measure(OutputFormat::TokeiJson, &WITH_BLANKS, TOKEI);
        assert_eq!(as_numbers(&tokei.counts, "blanks"), (13, 10, 2, 1));
    }

    #[test]
    fn regions_come_out_named_sorted_and_counted() {
        let tokei = measure(OutputFormat::TokeiJson, &WITH_BLANKS, TOKEI);
        let names: Vec<&str> = tokei.regions.iter().map(|r| r.language.as_str()).collect();
        assert_eq!(names, ["CSS", "HTML", "JavaScript"]);
        assert_eq!(as_numbers_of_region(&tokei.regions[1], "blanks"), (2, 2, 0, 0));
    }

    // A readme whose html fence holds a script. The JavaScript sits two levels down, where a
    // reader that stops at the first level loses it without a word.
    #[test]
    fn a_language_two_levels_down_is_read_out_of_the_blobs_and_not_lost() {
        let tokei = measure(OutputFormat::TokeiJson, &WITH_BLANKS, TOKEI_DEEP);
        assert_eq!(as_numbers(&tokei.counts, "blanks"), (12, 5, 5, 2));
        let names: Vec<&str> = tokei.regions.iter().map(|r| r.language.as_str()).collect();
        assert_eq!(names, ["HTML", "JavaScript"]);
        assert_eq!(as_numbers_of_region(&tokei.regions[0], "blanks"), (4, 4, 0, 0));
        assert_eq!(as_numbers_of_region(&tokei.regions[1], "blanks"), (2, 1, 1, 0));
    }

    #[test]
    fn a_counter_that_claims_nothing_is_not_a_counter_that_answered_zero() {
        let text = r#"{"Total":{"blanks":0,"code":0,"comments":0,"children":{}}}"#;
        assert!(read_output(OutputFormat::TokeiJson, &named(&WITH_BLANKS), text).unwrap().is_none());
    }

    #[test]
    fn the_uniform_format_is_read_as_printed_and_null_claims_nothing() {
        let text = r#"{"lines": 4, "buckets": {"code": 2, "comments": 1, "blanks": 1},
            "regions": [{"language": "CSS", "lines": 2,
                         "buckets": {"code": 1, "comments": 1, "blanks": 0}}]}"#;
        let uniform = measure(OutputFormat::LinejudgeJson, &WITH_BLANKS, text);
        assert_eq!(as_numbers(&uniform.counts, "blanks"), (4, 2, 1, 1));
        assert_eq!(as_numbers_of_region(&uniform.regions[0], "blanks"), (2, 1, 1, 0));
        let nothing = read_output(OutputFormat::LinejudgeJson, &named(&WITH_BLANKS), "null");
        assert!(nothing.unwrap().is_none());
    }

    // Every count is taken by its name, so four buckets read like three, and a bucket the counter
    // never printed is named beside the ones it did.
    #[test]
    fn a_bucket_a_counter_never_printed_is_refused_beside_what_it_did_print() {
        let four = ["code", "comments", "documentation", "blanks"];
        let text = r#"{"lines": 4, "buckets":
            {"code": 2, "comments": 0, "documentation": 1, "blanks": 1}}"#;
        let uniform = measure(OutputFormat::LinejudgeJson, &four, text);
        assert_eq!(uniform.counts.buckets["documentation"], 1);

        let refused = read_output(OutputFormat::TokeiJson, &named(&four), TOKEI).unwrap_err();
        assert!(refused.contains("printed no documentation"), "{refused}");
        assert!(refused.contains("blanks, code, comments"), "{refused}");
    }

    #[test]
    fn the_order_a_dialect_lists_its_buckets_in_changes_nothing() {
        let listed = measure(OutputFormat::TokeiJson, &["code", "comments", "blanks"], TOKEI);
        let shuffled = measure(OutputFormat::TokeiJson, &["blanks", "code", "comments"], TOKEI);
        assert_eq!(listed, shuffled);
        assert!(listed.counts.buckets["code"] > 0);
        assert!(!listed.regions.is_empty(), "and the regions came through it too");
    }

    #[test]
    fn a_bucket_this_dialect_has_not_is_refused_by_its_name() {
        let text = r#"{"lines": 4, "buckets": {"code": 2, "comments": 1, "blank": 1}}"#;
        let refused =
            read_output(OutputFormat::LinejudgeJson, &named(&WITH_BLANKS), text).unwrap_err();
        assert!(refused.contains("no blanks"), "{refused}");
    }

    #[test]
    fn output_that_does_not_parse_is_an_error_and_not_an_absent_answer() {
        let broken = read_output(OutputFormat::TokeiJson, &named(&WITH_BLANKS), "not json at all");
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
