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
    buckets: &[String],
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

fn read_mezura(buckets: &[String], text: &str) -> Result<Option<Answer>, String> {
    let run: MezuraRun = parse(text)?;
    let Some(language) = run.languages.first() else { return Ok(None) };
    let mut regions = Vec::new();
    for nested in &language.nested_languages {
        regions.push(RegionCounts {
            language: nested.name.clone(),
            lines: nested.counts.lines,
            buckets: read_buckets(&collect_mezura_counts(&nested.counts), buckets, "mezura")?,
        });
    }
    Ok(Some(Answer {
        counts: Counts {
            lines: run.total.lines,
            buckets: read_buckets(&collect_mezura_counts(&run.total), buckets, "mezura")?,
        },
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

fn read_scc(buckets: &[String], text: &str) -> Result<Option<Answer>, String> {
    let rows: Vec<SccRow> = parse(text)?;
    if rows.is_empty() {
        return Ok(None);
    }
    let (mut lines, mut code, mut comments, mut blanks) = (0u64, 0u64, 0u64, 0u64);
    for row in rows {
        lines += u64::from(row.lines);
        code += u64::from(row.code);
        comments += u64::from(row.comment);
        blanks += u64::from(row.blank);
    }
    let printed = BTreeMap::from([("code", code), ("comments", comments), ("blanks", blanks)]);
    Ok(Some(Answer {
        counts: Counts {
            lines: count_lines(lines, "the file")?,
            buckets: read_buckets(&printed, buckets, "scc")?,
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

// mezura names its third bucket after the model it counted with, so a document holds `extra` or
// `blanks` and never both.
fn collect_mezura_counts(counts: &MezuraCounts) -> BTreeMap<&str, u64> {
    let mut printed = BTreeMap::from([
        ("code", u64::from(counts.code)),
        ("comments", u64::from(counts.comments)),
    ]);
    for (name, number) in [("extra", counts.extra), ("blanks", counts.blanks)] {
        if let Some(number) = number {
            printed.insert(name, u64::from(number));
        }
    }
    printed
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
    extra: Option<u32>,
    blanks: Option<u32>,
}

#[derive(Deserialize)]
struct MezuraNested {
    name: String,
    #[serde(flatten)]
    counts: MezuraCounts,
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
    const MEZURA_REGION: &str = include_str!("../tests/fixtures/output/mezura-region.json");
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

    // mezura names its third bucket after the model it counted with, and both documents are the
    // same file counted both ways, so each dialect finds its own bucket and neither finds the
    // other's.
    #[test]
    fn each_way_mezura_counts_is_read_under_the_name_its_own_document_uses() {
        let content = measure(OutputFormat::MezuraJson, &WITH_EXTRA, MEZURA);
        let region = measure(OutputFormat::MezuraJson, &WITH_BLANKS, MEZURA_REGION);
        assert_eq!(as_numbers(&content.counts, "extra"), (13, 9, 3, 1));
        assert_eq!(as_numbers(&region.counts, "blanks"), (13, 9, 3, 1));
        assert_eq!(region.regions[1].buckets["blanks"], 1);

        let wrong = read_output(OutputFormat::MezuraJson, &named(&WITH_BLANKS), MEZURA).unwrap_err();
        assert!(wrong.contains("printed no blanks"), "{wrong}");
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
            assert!(read_output(output, &named(&WITH_BLANKS), text).unwrap().is_none());
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
        let nothing = read_output(OutputFormat::LinejudgeJson, &named(&WITH_BLANKS), "null");
        assert!(nothing.unwrap().is_none());
    }

    // A counter that sorts its lines into four buckets is read like any other, since every count is
    // taken by its name. What decides whether it can be read is what its own document holds, and a
    // bucket that is not in there is said out loud beside what is.
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

    // The guard the old shape could not have: it paired three numbers with three names in the order
    // they were listed, so this test would have passed with every count under the wrong name.
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
