use std::collections::BTreeMap;

use serde::Deserialize;

use crate::buckets::check_buckets;
use crate::corpus::{Counts, RegionCounts};

const TOKEI_TOTAL: &str = "Total";

#[derive(Debug)]
pub struct Measurement {
    pub counts: Counts,
    pub regions: Vec<RegionCounts>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum OutputFormat {
    #[serde(rename = "mezura-json")]
    MezuraJson,
    #[serde(rename = "tokei-json")]
    TokeiJson,
    #[serde(rename = "scc-json")]
    SccJson,
    // The exact format linejudge expects, so that it can construct the Measurement struct.
    // A counter tool can provide the format directly, instead of needing a transformer
    // function that will reshape the data it outputs to our requested format
    #[serde(rename = "linejudge-json")]
    LinejudgeJson,
}

/// `None` is a counter that does not claim the file, which is not the same answer as zeroes.
pub fn read_output(
    output: OutputFormat,
    third_bucket: &str,
    text: &str,
) -> Result<Option<Measurement>, String> {
    match output {
        OutputFormat::MezuraJson => read_mezura(third_bucket, text),
        OutputFormat::TokeiJson => read_tokei(third_bucket, text),
        OutputFormat::SccJson => read_scc(third_bucket, text),
        OutputFormat::LinejudgeJson => read_linejudge(third_bucket, text),
    }
}

// The format an adapter outside this repository prints, already in the shape the checker wants,
// with the buckets carrying the dialect's own names. `null` is a file the counter does not claim.
fn read_linejudge(third_bucket: &str, text: &str) -> Result<Option<Measurement>, String> {
    let raw: Option<LinejudgeMeasurement> = parse(text)?;
    let Some(raw) = raw else { return Ok(None) };
    let wanted = ["code", "comments", third_bucket];
    check_buckets(&raw.buckets, &wanted)?;
    let mut regions = Vec::new();
    for region in raw.regions {
        check_buckets(&region.buckets, &wanted)
            .map_err(|e| format!("in the {} region: {e}", region.language))?;
        regions.push(RegionCounts {
            language: region.language,
            lines: region.lines,
            buckets: region.buckets,
        });
    }
    Ok(Some(Measurement {
        counts: Counts { lines: raw.lines, buckets: raw.buckets },
        regions: sort_regions(regions),
    }))
}

fn read_mezura(third_bucket: &str, text: &str) -> Result<Option<Measurement>, String> {
    let run: MezuraRun = parse(text)?;
    let Some(language) = run.languages.first() else { return Ok(None) };
    // mezura names the third bucket `extra` in its document under either counting model, while the
    // model it ran under decides what that bucket is called here.
    let regions = language
        .nested_languages
        .iter()
        .map(|n| RegionCounts {
            language: n.name.clone(),
            lines: n.lines,
            buckets: buckets_of(n.code, n.comments, n.extra, third_bucket),
        })
        .collect();
    Ok(Some(Measurement {
        counts: Counts {
            lines: run.total.lines,
            buckets: buckets_of(run.total.code, run.total.comments, run.total.extra, third_bucket),
        },
        regions: sort_regions(regions),
    }))
}

fn read_tokei(third_bucket: &str, text: &str) -> Result<Option<Measurement>, String> {
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
    for (name, (code, comments, third)) in summed {
        regions.push(RegionCounts {
            lines: count_lines(code + comments + third, &name)?,
            buckets: buckets_of(
                count_lines(code, &name)?,
                count_lines(comments, &name)?,
                count_lines(third, &name)?,
                third_bucket,
            ),
            language: name,
        });
    }
    let whole = u64::from(total.code) + u64::from(total.comments) + u64::from(total.blanks);
    Ok(Some(Measurement {
        counts: Counts {
            lines: count_lines(whole, TOKEI_TOTAL)?,
            buckets: buckets_of(total.code, total.comments, total.blanks, third_bucket),
        },
        regions: sort_regions(regions),
    }))
}

fn read_scc(third_bucket: &str, text: &str) -> Result<Option<Measurement>, String> {
    let rows: Vec<SccRow> = parse(text)?;
    if rows.is_empty() {
        return Ok(None);
    }
    let (mut lines, mut code, mut comments, mut third) = (0u64, 0u64, 0u64, 0u64);
    for row in rows {
        lines += u64::from(row.lines);
        code += u64::from(row.code);
        comments += u64::from(row.comment);
        third += u64::from(row.blank);
    }
    Ok(Some(Measurement {
        counts: Counts {
            lines: count_lines(lines, "the file")?,
            buckets: buckets_of(
                count_lines(code, "the file")?,
                count_lines(comments, "the file")?,
                count_lines(third, "the file")?,
                third_bucket,
            ),
        },
        regions: Vec::new(),
    }))
}

// A counter is free to print a number for every line it claims to have seen; it is not free to
// claim more lines than a file can have, and a wrapped sum presented as its answer would be worse
// than saying so.
fn count_lines(counted: u64, whose: &str) -> Result<u32, String> {
    u32::try_from(counted).map_err(|_| format!("{whose} was counted as {counted} lines"))
}

fn buckets_of(code: u32, comments: u32, third: u32, third_bucket: &str) -> BTreeMap<String, u32> {
    BTreeMap::from([
        ("code".to_string(), code),
        ("comments".to_string(), comments),
        (third_bucket.to_string(), third),
    ])
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
struct LinejudgeMeasurement {
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
struct MezuraRun {
    total: MezuraCounts,
    languages: Vec<MezuraLanguage>,
}

#[derive(Deserialize)]
struct MezuraLanguage {
    #[serde(default)]
    nested_languages: Vec<MezuraNested>,
}

#[derive(Deserialize)]
struct MezuraCounts {
    lines: u32,
    code: u32,
    comments: u32,
    extra: u32,
}

#[derive(Deserialize)]
struct MezuraNested {
    name: String,
    lines: u32,
    code: u32,
    comments: u32,
    extra: u32,
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

#[derive(Deserialize)]
struct SccRow {
    #[serde(rename = "Lines")]
    lines: u32,
    #[serde(rename = "Code")]
    code: u32,
    #[serde(rename = "Comment")]
    comment: u32,
    #[serde(rename = "Blank")]
    blank: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    const MEZURA: &str = include_str!("../tests/fixtures/output/mezura-nested.json");
    const TOKEI: &str = include_str!("../tests/fixtures/output/tokei-nested.json");
    const SCC: &str = include_str!("../tests/fixtures/output/scc-nested.json");
    const TOKEI_DEEP: &str = include_str!("../tests/fixtures/output/tokei-three-levels.json");

    #[test]
    fn the_three_formats_agree_on_the_file_they_all_counted() {
        let mezura = measure(OutputFormat::MezuraJson, "extra", MEZURA);
        let tokei = measure(OutputFormat::TokeiJson, "blanks", TOKEI);
        let scc = measure(OutputFormat::SccJson, "blanks", SCC);
        for one in [&mezura, &tokei, &scc] {
            assert_eq!(one.counts.lines, 13);
        }
        assert_eq!(as_numbers(&mezura.counts, "extra"), (13, 9, 3, 1));
        assert_eq!(as_numbers(&tokei.counts, "blanks"), (13, 10, 2, 1));
        assert_eq!(as_numbers(&scc.counts, "blanks"), (13, 13, 0, 0));
    }

    #[test]
    fn regions_come_out_named_sorted_and_counted() {
        let mezura = measure(OutputFormat::MezuraJson, "extra", MEZURA);
        let names: Vec<&str> = mezura.regions.iter().map(|r| r.language.as_str()).collect();
        assert_eq!(names, ["CSS", "JavaScript"]);
        assert_eq!(mezura.regions[1].lines, 3);
        assert_eq!(mezura.regions[1].buckets["extra"], 1);

        let tokei = measure(OutputFormat::TokeiJson, "blanks", TOKEI);
        let names: Vec<&str> = tokei.regions.iter().map(|r| r.language.as_str()).collect();
        assert_eq!(names, ["CSS", "HTML", "JavaScript"]);
        assert_eq!(as_numbers_of_region(&tokei.regions[1], "blanks"), (2, 2, 0, 0));

        assert!(measure(OutputFormat::SccJson, "blanks", SCC).regions.is_empty());
    }

    // Captured from a readme whose html fence holds a script: Markdown 6, HTML 4, JavaScript 2.
    // The JavaScript sits two levels down, inside the HTML child's own blobs, which is where a
    // reader that stops at the first level silently loses it.
    #[test]
    fn a_language_two_levels_down_is_read_out_of_the_blobs_and_not_lost() {
        let tokei = measure(OutputFormat::TokeiJson, "blanks", TOKEI_DEEP);
        assert_eq!(as_numbers(&tokei.counts, "blanks"), (12, 5, 5, 2));
        let names: Vec<&str> = tokei.regions.iter().map(|r| r.language.as_str()).collect();
        assert_eq!(names, ["HTML", "JavaScript"]);
        assert_eq!(as_numbers_of_region(&tokei.regions[0], "blanks"), (4, 4, 0, 0));
        assert_eq!(as_numbers_of_region(&tokei.regions[1], "blanks"), (2, 1, 1, 0));
    }

    #[test]
    fn a_counter_that_claims_nothing_is_not_a_counter_that_answered_zero() {
        let nothing = [
            (OutputFormat::MezuraJson, r#"{"total":{"lines":0,"code":0,"comments":0,"extra":0},"languages":[]}"#),
            (OutputFormat::TokeiJson, r#"{"Total":{"blanks":0,"code":0,"comments":0,"children":{}}}"#),
            (OutputFormat::SccJson, "[]"),
        ];
        for (output, text) in nothing {
            assert!(read_output(output, "blanks", text).unwrap().is_none());
        }
    }

    #[test]
    fn the_uniform_format_is_read_as_printed_and_null_claims_nothing() {
        let text = r#"{"lines": 4, "buckets": {"code": 2, "comments": 1, "blanks": 1},
            "regions": [{"language": "CSS", "lines": 2,
                         "buckets": {"code": 1, "comments": 1, "blanks": 0}}]}"#;
        let uniform = measure(OutputFormat::LinejudgeJson, "blanks", text);
        assert_eq!(as_numbers(&uniform.counts, "blanks"), (4, 2, 1, 1));
        assert_eq!(as_numbers_of_region(&uniform.regions[0], "blanks"), (2, 1, 1, 0));
        assert!(read_output(OutputFormat::LinejudgeJson, "blanks", "null").unwrap().is_none());
    }

    #[test]
    fn a_bucket_this_dialect_has_not_is_refused_by_its_name() {
        let text = r#"{"lines": 4, "buckets": {"code": 2, "comments": 1, "blank": 1}}"#;
        let refused = read_output(OutputFormat::LinejudgeJson, "blanks", text).unwrap_err();
        assert!(refused.contains("no blanks"), "{refused}");
    }

    #[test]
    fn output_that_does_not_parse_is_an_error_and_not_an_absent_answer() {
        let broken = read_output(OutputFormat::TokeiJson, "blanks", "not json at all");
        assert!(broken.unwrap_err().contains("does not parse"));
    }

    fn measure(output: OutputFormat, third: &str, text: &str) -> Measurement {
        read_output(output, third, text).unwrap().unwrap()
    }

    fn as_numbers(counts: &Counts, third: &str) -> (u32, u32, u32, u32) {
        (counts.lines, counts.buckets["code"], counts.buckets["comments"], counts.buckets[third])
    }

    fn as_numbers_of_region(region: &RegionCounts, third: &str) -> (u32, u32, u32, u32) {
        (region.lines, region.buckets["code"], region.buckets["comments"], region.buckets[third])
    }
}
