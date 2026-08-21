use std::collections::BTreeMap;

use linejudge::answer;
use serde::{Deserialize, Serialize};

use crate::marks::Ink;

// One measurement of the whole roster, in the shape it is published. The pages are rendered from
// this and `data.json` is this, so a field here is a promise to whoever reads that file. Nothing
// of the library reaches a page except through here.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Sweep {
    pub measured_on: String,
    pub groups: Vec<Group>,
    pub counters: Vec<Counter>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Group {
    pub name: String,
    pub cases: Vec<Case>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Case {
    pub name: String,
    // Empty for a disabled case, whose files are never read.
    pub trap: String,
    pub disabled: bool,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Counter {
    pub name: String,
    pub version: String,
    pub dialects: Vec<Dialect>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Dialect {
    pub name: String,
    pub answers: Vec<Answer>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Answer {
    pub case: String,
    pub verdict: Verdict,
    // What the counter's own rules ask for. `None` only where it broke, taking the derivation
    // with it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub wants: Option<Counts>,
    // What it answered. `None` where it claims no such file, or broke.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub answered: Option<Counts>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub wants_regions: Vec<Region>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub answered_regions: Vec<Region>,
    // The recorded note, carried only while the answer it was written about is the answer given.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub exception: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub broke: Option<String>,
    pub command: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Verdict {
    Agrees,
    Fails,
    Unclaimed,
    Broke,
}

// Everything a case's own page shows that the measurement does not carry: the file itself, the
// spans marked in it, and how each way of counting reads every line. Deliberately outside the
// published JSON, which holds what the tools answered and not a copy of the corpus.
#[derive(Debug, PartialEq)]
pub struct CaseDetail {
    pub name: String,
    pub group: String,
    pub trap: String,
    pub file: String,
    // Every way of counting the page speaks about, as `counter.dialect`, in the order the
    // scoreboard shows them, which is the order every line's readings are in.
    pub ways: Vec<String>,
    pub lines: Vec<Line>,
}

#[derive(Debug, PartialEq)]
pub struct Line {
    pub pieces: Vec<Piece>,
    pub counted: Vec<Counted>,
}

// A stretch of one line and what covers it, so a page can paint the file without knowing what a
// marker looks like.
#[derive(Debug, PartialEq)]
pub struct Piece {
    pub ink: Ink,
    pub text: String,
}

// Where one way of counting puts one line, and which of its rules put it there.
#[derive(Debug, PartialEq)]
pub struct Counted {
    pub bucket: String,
    pub rules: Vec<String>,
    pub region: Option<String>,
}

// One counter as its own page shows it: the half of the measurement that has nothing to do with
// any case.
#[derive(Debug, PartialEq)]
pub struct ToolDetail {
    pub name: String,
    pub version: String,
    // `None` where the adapter does not say.
    pub repository: Option<String>,
    // `None` for a counter that cannot be fetched.
    pub channel: Option<String>,
    pub dialects: Vec<DialectDetail>,
}

#[derive(Debug, PartialEq)]
pub struct DialectDetail {
    pub name: String,
    // What is put on the counter's command line to ask for this way of counting, which is the only
    // thing that says what the name of it means. Empty for a counter that has just the one.
    pub flags: Vec<String>,
    pub rules: Vec<RuleDetail>,
}

// One rule of a dialect, its conditions already written out in words for somebody who will never
// open the file they came from.
#[derive(Debug, PartialEq)]
pub struct RuleDetail {
    pub name: String,
    pub bucket: String,
    pub when: Vec<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Counts {
    pub lines: u32,
    #[serde(flatten)]
    pub buckets: BTreeMap<String, u32>,
}

impl Counts {
    pub fn of(counts: &answer::Counts) -> Counts {
        Counts { lines: counts.lines, buckets: counts.buckets.clone() }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub language: String,
    pub lines: u32,
    #[serde(flatten)]
    pub buckets: BTreeMap<String, u32>,
}

impl Region {
    pub fn of(region: &answer::RegionCounts) -> Region {
        Region {
            language: region.language.clone(),
            lines: region.lines,
            buckets: region.buckets.clone(),
        }
    }

    pub fn to_counts(&self) -> Counts {
        Counts { lines: self.lines, buckets: self.buckets.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sweep_survives_the_trip_through_its_own_json() {
        let sweep = Sweep {
            measured_on: "2026-08-20".to_string(),
            groups: vec![Group {
                name: "1000-comments".to_string(),
                cases: vec![
                    Case {
                        name: "1010-a_case".to_string(),
                        trap: "a trap".to_string(),
                        disabled: false,
                    },
                    Case {
                        name: "1020-set_aside".to_string(),
                        trap: String::new(),
                        disabled: true,
                    },
                ],
            }],
            counters: vec![Counter {
                name: "tokei".to_string(),
                version: "tokei 14.0.0".to_string(),
                dialects: vec![Dialect {
                    name: "default".to_string(),
                    answers: vec![Answer {
                        case: "1010-a_case".to_string(),
                        verdict: Verdict::Fails,
                        wants: Some(Counts {
                            lines: 2,
                            buckets: BTreeMap::from([("code".to_string(), 2)]),
                        }),
                        answered: Some(Counts {
                            lines: 2,
                            buckets: BTreeMap::from([("code".to_string(), 1)]),
                        }),
                        wants_regions: vec![Region {
                            language: "CSS".to_string(),
                            lines: 2,
                            buckets: BTreeMap::from([("code".to_string(), 2)]),
                        }],
                        answered_regions: Vec::new(),
                        note: Some("a note".to_string()),
                        exception: None,
                        broke: None,
                        command: "tokei cases/1000-comments/1010-a_case/input.c".to_string(),
                    }],
                }],
            }],
        };
        let text = serde_json::to_string_pretty(&sweep).unwrap();
        let back: Sweep = serde_json::from_str(&text).unwrap();
        assert_eq!(back, sweep);
        assert!(text.contains("\"measured-on\""), "{text}");
        assert!(text.contains("\"wants-regions\""), "{text}");
        assert!(!text.contains("\"broke\""), "an absent field is not written\n{text}");
    }

    #[test]
    fn the_buckets_sit_flat_beside_the_lines_the_way_the_recorded_files_write_them() {
        let counts = Counts {
            lines: 5,
            buckets: BTreeMap::from([("code".to_string(), 2), ("comments".to_string(), 3)]),
        };
        let text = serde_json::to_string(&counts).unwrap();
        assert_eq!(text, "{\"lines\":5,\"code\":2,\"comments\":3}");
    }
}
