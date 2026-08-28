use std::collections::BTreeMap;

use serde::Deserialize;

use crate::answer::{Answer, Counts, RegionCounts};
use crate::measurement::{count_lines, parse, sort_regions};

const TOKEI_TOTAL: &str = "Total";

pub fn read_counts(buckets: &[String], text: &str) -> Result<Option<Answer>, String> {
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
            buckets: read_buckets(&printed, buckets)?,
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
            buckets: read_buckets(&printed, buckets)?,
        },
        regions: sort_regions(regions),
    }))
}

fn read_buckets(
    printed: &BTreeMap<&str, u64>,
    wanted: &[String],
) -> Result<BTreeMap<String, u32>, String> {
    let mut counts = BTreeMap::new();
    for bucket in wanted {
        let Some(number) = printed.get(bucket.as_str()) else {
            let named: Vec<&str> = printed.keys().copied().collect();
            return Err(format!(
                "tokei printed no {bucket} for this file, it printed {}",
                named.join(", ")
            ));
        };
        counts.insert(bucket.clone(), count_lines(*number, &format!("tokei's {bucket}"))?);
    }
    Ok(counts)
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

    const TOKEI: &str = include_str!("../../tests/fixtures/output/tokei-nested.json");
    const TOKEI_DEEP: &str = include_str!("../../tests/fixtures/output/tokei-three-levels.json");
    const WITH_BLANKS: [&str; 3] = ["code", "comments", "blanks"];

    #[test]
    fn tokei_is_read_with_its_lines_as_the_three_buckets_added_up() {
        let tokei = measure(&WITH_BLANKS, TOKEI);
        assert_eq!(as_numbers(&tokei.counts, "blanks"), (13, 10, 2, 1));
    }

    #[test]
    fn regions_come_out_named_sorted_and_counted() {
        let tokei = measure(&WITH_BLANKS, TOKEI);
        let names: Vec<&str> = tokei.regions.iter().map(|r| r.language.as_str()).collect();
        assert_eq!(names, ["CSS", "HTML", "JavaScript"]);
        assert_eq!(as_numbers_of_region(&tokei.regions[1], "blanks"), (2, 2, 0, 0));
    }

    // A readme whose html fence holds a script, so the JavaScript sits two levels down.
    #[test]
    fn a_language_two_levels_down_is_read_out_of_the_blobs_and_not_lost() {
        let tokei = measure(&WITH_BLANKS, TOKEI_DEEP);
        assert_eq!(as_numbers(&tokei.counts, "blanks"), (12, 5, 5, 2));
        let names: Vec<&str> = tokei.regions.iter().map(|r| r.language.as_str()).collect();
        assert_eq!(names, ["HTML", "JavaScript"]);
        assert_eq!(as_numbers_of_region(&tokei.regions[0], "blanks"), (4, 4, 0, 0));
        assert_eq!(as_numbers_of_region(&tokei.regions[1], "blanks"), (2, 1, 1, 0));
    }

    #[test]
    fn a_bucket_tokei_never_printed_is_refused_beside_what_it_did_print() {
        let four = ["code", "comments", "documentation", "blanks"];
        let refused = read_counts(&named(&four), TOKEI).unwrap_err();
        assert!(refused.contains("printed no documentation"), "{refused}");
        assert!(refused.contains("blanks, code, comments"), "{refused}");
    }

    fn measure(buckets: &[&str], text: &str) -> Answer {
        read_counts(&named(buckets), text).unwrap().unwrap()
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
