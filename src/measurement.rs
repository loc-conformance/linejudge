use std::collections::BTreeMap;

use serde::Deserialize;

use crate::answer::{Answer, Counts, RegionCounts};
use crate::dialects::check_buckets;

const TOKEI_TOTAL: &str = "Total";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum OutputFormat {
    #[serde(rename = "mezura-json")]
    MezuraJson,
    #[serde(rename = "tokei-json")]
    TokeiJson,
    #[serde(rename = "scc-json")]
    SccJson,
    // The shape linejudge reads without translating anything. A counter can print this itself and
    // needs no reader of its own in here, whatever its buckets are called and however many it has.
    #[serde(rename = "linejudge-json")]
    LinejudgeJson,
}

/// `buckets` are the names this way of counting gives its buckets, and every count read here comes
/// back under one of them. `None` is a counter that does not claim the file, which is not the same
/// answer as zeroes.
pub fn read_output(
    output: OutputFormat,
    buckets: &[&str],
    text: &str,
) -> Result<Option<Answer>, String> {
    match output {
        OutputFormat::MezuraJson => read_mezura(buckets, text),
        OutputFormat::TokeiJson => read_tokei(buckets, text),
        OutputFormat::SccJson => read_scc(buckets, text),
        OutputFormat::LinejudgeJson => read_linejudge(buckets, text),
    }
}

// The format an adapter outside this repository prints, already in the shape the checker wants,
// with the buckets carrying the dialect's own names. `null` is a file the counter does not claim.
fn read_linejudge(buckets: &[&str], text: &str) -> Result<Option<Answer>, String> {
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

fn read_mezura(buckets: &[&str], text: &str) -> Result<Option<Answer>, String> {
    let run: MezuraRun = parse(text)?;
    let Some(language) = run.languages.first() else { return Ok(None) };
    // mezura names its third bucket `extra` in the document under either counting model, while the
    // model it ran under decides what that bucket is called here.
    let mut regions = Vec::new();
    for nested in &language.nested_languages {
        regions.push(RegionCounts {
            language: nested.name.clone(),
            lines: nested.lines,
            buckets: name_three_counts(buckets, nested.code, nested.comments, nested.extra)?,
        });
    }
    Ok(Some(Answer {
        counts: Counts {
            lines: run.total.lines,
            buckets: name_three_counts(
                buckets,
                run.total.code,
                run.total.comments,
                run.total.extra,
            )?,
        },
        regions: sort_regions(regions),
    }))
}

fn read_tokei(buckets: &[&str], text: &str) -> Result<Option<Answer>, String> {
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
            buckets: name_three_counts(
                buckets,
                count_lines(code, &name)?,
                count_lines(comments, &name)?,
                count_lines(third, &name)?,
            )?,
            language: name,
        });
    }
    let whole = u64::from(total.code) + u64::from(total.comments) + u64::from(total.blanks);
    Ok(Some(Answer {
        counts: Counts {
            lines: count_lines(whole, TOKEI_TOTAL)?,
            buckets: name_three_counts(buckets, total.code, total.comments, total.blanks)?,
        },
        regions: sort_regions(regions),
    }))
}

fn read_scc(buckets: &[&str], text: &str) -> Result<Option<Answer>, String> {
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
    Ok(Some(Answer {
        counts: Counts {
            lines: count_lines(lines, "the file")?,
            buckets: name_three_counts(
                buckets,
                count_lines(code, "the file")?,
                count_lines(comments, "the file")?,
                count_lines(third, "the file")?,
            )?,
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

// The three JSON documents above each hold exactly three counts of lines, in that order: what the
// tool called code, what it called comments, and what it called everything else. The names they go
// under are the ones the way of counting declares, so a counter that sorts its lines into some
// other number of buckets cannot be read out of one of these three documents at all.
fn name_three_counts(
    names: &[&str],
    code: u32,
    comments: u32,
    third: u32,
) -> Result<BTreeMap<String, u32>, String> {
    match names {
        [first, second, last] => Ok(BTreeMap::from([
            ((*first).to_string(), code),
            ((*second).to_string(), comments),
            ((*last).to_string(), third),
        ])),
        _ => Err(format!(
            "this output holds three counts of lines and this way of counting has {}: {}",
            names.len(),
            names.join(", ")
        )),
    }
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
    const SCC: &str = include_str!("../tests/fixtures/output/scc-nested.json");
    const TOKEI: &str = include_str!("../tests/fixtures/output/tokei-nested.json");
    const TOKEI_DEEP: &str = include_str!("../tests/fixtures/output/tokei-three-levels.json");
    const WITH_BLANKS: [&str; 3] = ["code", "comments", "blanks"];
    const WITH_EXTRA: [&str; 3] = ["code", "comments", "extra"];

    #[test]
    fn the_three_formats_agree_on_the_file_they_all_counted() {
        let mezura = measure(OutputFormat::MezuraJson, &WITH_EXTRA, MEZURA);
        let tokei = measure(OutputFormat::TokeiJson, &WITH_BLANKS, TOKEI);
        let scc = measure(OutputFormat::SccJson, &WITH_BLANKS, SCC);
        for one in [&mezura, &tokei, &scc] {
            assert_eq!(one.counts.lines, 13);
        }
        assert_eq!(as_numbers(&mezura.counts, "extra"), (13, 9, 3, 1));
        assert_eq!(as_numbers(&tokei.counts, "blanks"), (13, 10, 2, 1));
        assert_eq!(as_numbers(&scc.counts, "blanks"), (13, 13, 0, 0));
    }

    #[test]
    fn regions_come_out_named_sorted_and_counted() {
        let mezura = measure(OutputFormat::MezuraJson, &WITH_EXTRA, MEZURA);
        let names: Vec<&str> = mezura.regions.iter().map(|r| r.language.as_str()).collect();
        assert_eq!(names, ["CSS", "JavaScript"]);
        assert_eq!(mezura.regions[1].lines, 3);
        assert_eq!(mezura.regions[1].buckets["extra"], 1);

        let tokei = measure(OutputFormat::TokeiJson, &WITH_BLANKS, TOKEI);
        let names: Vec<&str> = tokei.regions.iter().map(|r| r.language.as_str()).collect();
        assert_eq!(names, ["CSS", "HTML", "JavaScript"]);
        assert_eq!(as_numbers_of_region(&tokei.regions[1], "blanks"), (2, 2, 0, 0));

        assert!(measure(OutputFormat::SccJson, &WITH_BLANKS, SCC).regions.is_empty());
    }

    // Captured from a readme whose html fence holds a script: Markdown 6, HTML 4, JavaScript 2.
    // The JavaScript sits two levels down, inside the HTML child's own blobs, which is where a
    // reader that stops at the first level silently loses it.
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
        let nothing = [
            (OutputFormat::MezuraJson, r#"{"total":{"lines":0,"code":0,"comments":0,"extra":0},"languages":[]}"#),
            (OutputFormat::TokeiJson, r#"{"Total":{"blanks":0,"code":0,"comments":0,"children":{}}}"#),
            (OutputFormat::SccJson, "[]"),
        ];
        for (output, text) in nothing {
            assert!(read_output(output, &WITH_BLANKS, text).unwrap().is_none());
        }
    }

    #[test]
    fn the_uniform_format_is_read_as_printed_and_null_claims_nothing() {
        let text = r#"{"lines": 4, "buckets": {"code": 2, "comments": 1, "blanks": 1},
            "regions": [{"language": "CSS", "lines": 2,
                         "buckets": {"code": 1, "comments": 1, "blanks": 0}}]}"#;
        let uniform = measure(OutputFormat::LinejudgeJson, &WITH_BLANKS, text);
        assert_eq!(as_numbers(&uniform.counts, "blanks"), (4, 2, 1, 1));
        assert_eq!(as_numbers_of_region(&uniform.regions[0], "blanks"), (2, 1, 1, 0));
        let nothing = read_output(OutputFormat::LinejudgeJson, &WITH_BLANKS, "null");
        assert!(nothing.unwrap().is_none());
    }

    // The uniform format carries every count under a name, so a counter that sorts its lines into
    // four buckets is readable there. The three formats above hold three counts and no name at
    // all, and that is the one place where the number three is a fact rather than a habit.
    #[test]
    fn four_buckets_are_read_out_of_the_uniform_format_and_out_of_no_other() {
        let four = ["code", "comments", "documentation", "blanks"];
        let text = r#"{"lines": 4, "buckets":
            {"code": 2, "comments": 0, "documentation": 1, "blanks": 1}}"#;
        let uniform = measure(OutputFormat::LinejudgeJson, &four, text);
        assert_eq!(uniform.counts.buckets["documentation"], 1);

        let refused = read_output(OutputFormat::TokeiJson, &four, TOKEI).unwrap_err();
        assert!(refused.contains("three counts of lines"), "{refused}");
        assert!(refused.contains("has 4"), "{refused}");
    }

    #[test]
    fn a_bucket_this_dialect_has_not_is_refused_by_its_name() {
        let text = r#"{"lines": 4, "buckets": {"code": 2, "comments": 1, "blank": 1}}"#;
        let refused = read_output(OutputFormat::LinejudgeJson, &WITH_BLANKS, text).unwrap_err();
        assert!(refused.contains("no blanks"), "{refused}");
    }

    #[test]
    fn output_that_does_not_parse_is_an_error_and_not_an_absent_answer() {
        let broken = read_output(OutputFormat::TokeiJson, &WITH_BLANKS, "not json at all");
        assert!(broken.unwrap_err().contains("does not parse"));
    }

    fn measure(output: OutputFormat, buckets: &[&str], text: &str) -> Answer {
        read_output(output, buckets, text).unwrap().unwrap()
    }

    fn as_numbers(counts: &Counts, third: &str) -> (u32, u32, u32, u32) {
        (counts.lines, counts.buckets["code"], counts.buckets["comments"], counts.buckets[third])
    }

    fn as_numbers_of_region(region: &RegionCounts, third: &str) -> (u32, u32, u32, u32) {
        (region.lines, region.buckets["code"], region.buckets["comments"], region.buckets[third])
    }
}
