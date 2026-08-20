use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use linejudge::adapter::{Adapter, Dialect};
use linejudge::corpus::{Case, Corpus};
use linejudge::deriver::explain_every_line;
use linejudge::dialects::Dialects;
use linejudge::recorded::RecordedAnswers;
use linejudge::verdict::{Conformance, Judged, Outcome, measure_and_judge_every_case};

use crate::marks::cut_into_stretches;
use crate::render::data::{self, Verdict};
use crate::style;

const GROUP_SIZE: u32 = 1000;
const SECONDS_PER_DAY: u64 = 86_400;

/// Measures the whole roster and answers with what it found, never with an exit code: a failure is
/// a finding the sweep carries, and only this suite's own data refusing to judge is an error. A
/// counter with no binary is named on stderr and left out.
pub fn measure_every_counter(
    adapters: &[Adapter],
    corpus: &Corpus,
    dialects: &Dialects,
    recorded: &[PathBuf],
    find_binary: &dyn Fn(&str) -> Option<PathBuf>,
) -> Result<data::Sweep, String> {
    let mut counters = Vec::new();
    for adapter in adapters {
        let name = &adapter.name_of_counter;
        let Some(binary) = find_binary(name) else {
            eprintln!("{}", style::RECORDED.paint(&format!(
                    "{name}: no binary named for it, left out of the sweep")));
            continue;
        };
        let version = adapter.read_version_or_unknown(&binary);
        let record = RecordedAnswers::read(recorded, name, dialects)
            .map_err(|faults| faults.join("\n"))?;
        let mut measured = Vec::new();
        for dialect in &adapter.dialects {
            let Some(rules) = dialects.find(name, &dialect.name) else {
                return Err(format!("{name}.{} names no dialect file to judge by", dialect.name));
            };
            let judged = measure_and_judge_every_case(
                adapter,
                dialect,
                rules,
                &binary,
                corpus,
                record.as_ref(),
                &version,
            )
            .map_err(|faults| faults.join("\n"))?;
            let answers = judged
                .iter()
                .map(|one| {
                    build_one_answer(one, format_the_command_for(adapter, dialect, corpus, one.case))
                })
                .collect();
            measured.push(data::Dialect { name: dialect.name.clone(), answers });
        }
        counters.push(data::Counter { name: name.clone(), version, dialects: measured });
    }
    if counters.is_empty() {
        return Err("no counter was run, so there is nothing to publish: name binaries in \
                    .linejudge/counters.toml beside the project"
            .to_string());
    }
    Ok(data::Sweep {
        measured_on: format_the_utc_date(),
        groups: collect_the_groups_of(corpus),
        counters,
    })
}

/// Reads every case the way the pages under the scoreboard show it, which needs no binary: the
/// file, its marked spans, and each way of counting's own reading of every line. The ways are the
/// ones the sweep measured, so a counter left out for want of a binary is left out here too.
pub fn read_every_case(
    sweep: &data::Sweep,
    corpus: &Corpus,
    dialects: &Dialects,
) -> Result<Vec<data::CaseDetail>, String> {
    let mut ways = Vec::new();
    for counter in &sweep.counters {
        for dialect in &counter.dialects {
            let Some(rules) = dialects.find(&counter.name, &dialect.name) else {
                return Err(format!("{}.{} names no dialect file", counter.name, dialect.name));
            };
            ways.push((format!("{}.{}", counter.name, dialect.name), rules));
        }
    }

    let mut detailed = Vec::with_capacity(corpus.cases.len());
    for case in &corpus.cases {
        let mut read_by_way = Vec::with_capacity(ways.len());
        for (way, rules) in &ways {
            let explained = explain_every_line(&case.truth, rules, &corpus.readings)
                .map_err(|faults| format!("{}, {way}: {}", case.name, faults.join("; ")))?;
            read_by_way.push(explained);
        }
        let lines = case
            .truth
            .lines
            .iter()
            .enumerate()
            .map(|(at, line)| data::Line {
                pieces: cut_into_stretches(&line.source, &line.marker)
                    .into_iter()
                    .map(|(ink, text)| data::Piece { ink, text })
                    .collect(),
                counted: read_by_way
                    .iter()
                    .map(|explained| data::Counted {
                        bucket: explained[at].bucket.clone(),
                        rules: explained[at].rules.clone(),
                        region: explained[at].region.clone(),
                    })
                    .collect(),
            })
            .collect();
        detailed.push(data::CaseDetail {
            name: case.name.clone(),
            group: find_the_group_of(&case.name, &corpus.groups).unwrap_or_default().to_string(),
            trap: case.trap.clone(),
            file: case
                .input_file
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            ways: ways.iter().map(|(way, _)| way.clone()).collect(),
            lines,
        });
    }
    Ok(detailed)
}

fn build_one_answer(judged: &Judged, command: String) -> data::Answer {
    let case = judged.case.name.clone();
    let measured = match &judged.outcome {
        Outcome::Broke(message) => {
            return data::Answer {
                case,
                verdict: Verdict::Broke,
                wants: None,
                answered: None,
                wants_regions: Vec::new(),
                answered_regions: Vec::new(),
                note: None,
                exception: None,
                broke: Some(message.clone()),
                command,
            };
        }
        Outcome::Measured(measured) => measured,
    };
    let verdict = match measured.conformance {
        Conformance::Agrees => Verdict::Agrees,
        Conformance::Fails => Verdict::Fails,
        Conformance::Unclaimed => Verdict::Unclaimed,
    };
    let note = measured.record.and_then(|entry| match entry.counted == measured.live {
        true => entry.note.clone(),
        false => None,
    });
    data::Answer {
        case,
        verdict,
        wants: Some(data::Counts::of(&measured.real.counts)),
        answered: measured.live.as_ref().map(|live| data::Counts::of(&live.counts)),
        wants_regions: measured.real.regions.iter().map(data::Region::of).collect(),
        answered_regions: measured
            .live
            .as_ref()
            .map(|live| live.regions.iter().map(data::Region::of).collect())
            .unwrap_or_default(),
        note,
        exception: measured.exception.map(|exception| exception.note.clone()),
        broke: None,
        command,
    }
}

/// The command as anybody can retype it: the counter by its bare name and the case by its path
/// inside the corpus, never the local paths this run happened to resolve.
fn format_the_command_for(
    adapter: &Adapter,
    dialect: &Dialect,
    corpus: &Corpus,
    case: &Case,
) -> String {
    let input = case
        .input_file
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file = match find_the_group_of(&case.name, &corpus.groups) {
        Some(group) => format!("cases/{group}/{}/{input}", case.name),
        None => format!("cases/{}/{input}", case.name),
    };
    adapter.format_command(dialect, Path::new(&adapter.name_of_counter), Path::new(&file))
}

fn collect_the_groups_of(corpus: &Corpus) -> Vec<data::Group> {
    corpus
        .groups
        .iter()
        .map(|group| {
            let mut cases: Vec<data::Case> = corpus
                .cases
                .iter()
                .filter(|case| is_in_the_group(&case.name, group))
                .map(|case| data::Case {
                    name: case.name.clone(),
                    trap: case.trap.clone(),
                    disabled: false,
                })
                .collect();
            cases.extend(corpus.disabled.iter().filter(|name| is_in_the_group(name, group)).map(
                |name| data::Case { name: name.clone(), trap: String::new(), disabled: true },
            ));
            cases.sort_by_key(|case| (find_the_number_in(&case.name), case.name.clone()));
            data::Group { name: group.clone(), cases }
        })
        .collect()
}

fn find_the_group_of<'a>(case: &str, groups: &'a [String]) -> Option<&'a str> {
    groups.iter().find(|group| is_in_the_group(case, group)).map(String::as_str)
}

fn is_in_the_group(case: &str, group: &str) -> bool {
    match (find_the_number_in(case), find_the_number_in(group)) {
        (Some(number), Some(first)) => {
            (first..first.saturating_add(GROUP_SIZE)).contains(&number)
        }
        _ => false,
    }
}

fn find_the_number_in(name: &str) -> Option<u32> {
    name.split('-').next()?.parse().ok()
}

fn format_the_utc_date() -> String {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
    format_the_day_of(seconds)
}

// Howard Hinnant's civil-from-days: whole days since 1970 to a Gregorian date, no local time
// anywhere in it.
fn format_the_day_of(seconds_since_epoch: u64) -> String {
    let z = seconds_since_epoch / SECONDS_PER_DAY + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::env;
    use std::fs;

    use linejudge::answer::{Answer, Counts};
    use linejudge::recorded::RecordedAnswer;
    use linejudge::truth::Truth;
    use linejudge::verdict::Measured;

    use super::*;

    #[test]
    fn whole_days_since_the_epoch_come_out_as_the_dates_they_are() {
        assert_eq!(format_the_day_of(0), "1970-01-01");
        assert_eq!(format_the_day_of(951_782_400), "2000-02-29");
        assert_eq!(format_the_day_of(1_787_184_000 + 3_600), "2026-08-20");
    }

    #[test]
    fn the_note_is_carried_exactly_while_the_answer_it_was_written_about_is_the_answer_given() {
        let same = build_one_answer(&judged(5, Some(5)), String::new());
        assert_eq!(same.note.as_deref(), Some("a note"));
        assert_eq!(same.verdict, Verdict::Fails);

        let moved = build_one_answer(&judged(5, Some(4)), String::new());
        assert_eq!(moved.note, None, "the answer moved, so the note describes nothing");

        let gone = build_one_answer(&judged(5, None), String::new());
        assert_eq!(gone.note, None);
        assert_eq!(gone.verdict, Verdict::Unclaimed);
        assert_eq!(gone.answered, None);
    }

    #[test]
    fn a_case_the_counter_broke_on_carries_the_message_and_no_numbers() {
        let case = a_case("0400-a_case_built_by_a_test");
        let broke = Judged { case: &case, outcome: Outcome::Broke("exit status 101".to_string()) };
        let answer = build_one_answer(&broke, "tokei cases/x".to_string());
        assert_eq!(answer.verdict, Verdict::Broke);
        assert_eq!(answer.broke.as_deref(), Some("exit status 101"));
        assert_eq!(answer.wants, None);
        assert_eq!(answer.command, "tokei cases/x");
    }

    #[test]
    fn the_groups_of_a_sweep_hold_their_own_cases_and_their_own_disabled() {
        let root = env::temp_dir().join("linejudge-sweep_groups");
        let _ = fs::remove_dir_all(&root);
        for (group, case) in [
            ("0000-a_group_built_by_a_test", "0400-a_case_built_by_a_test"),
            ("1000-another_group", "1400-another_case"),
            ("1000-another_group", "disabled-1500-set_aside"),
        ] {
            let dir = root.join(group).join(case);
            fs::create_dir_all(&dir).unwrap();
            if case.starts_with("disabled-") {
                continue;
            }
            fs::write(dir.join("input.c"), "/* a block\n*/ int x = 1;\n").unwrap();
            fs::write(dir.join("truth.txt"), TRUTH).unwrap();
            fs::write(dir.join("case.toml"), "trap = \"\"\"\na block\"\"\"\n").unwrap();
        }
        let corpus = Corpus::read(&root).unwrap_or_else(|faults| panic!("{faults:?}"));
        let groups = collect_the_groups_of(&corpus);
        fs::remove_dir_all(&root).unwrap();

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].name, "0000-a_group_built_by_a_test");
        assert_eq!(groups[0].cases.len(), 1);
        assert!(!groups[0].cases[0].trap.is_empty());
        let names: Vec<&str> = groups[1].cases.iter().map(|case| case.name.as_str()).collect();
        assert_eq!(names, ["1400-another_case", "1500-set_aside"]);
        assert!(groups[1].cases[1].disabled);
        assert!(groups[1].cases[1].trap.is_empty());
    }

    const TRUTH: &str = "/* a block\nCCcccccccc\n*/ int x = 1;\nUU ... . . ..\n";

    // The rules want 3, so a live answer of 5 or 4 fails and its record was photographed at 5:
    // whether the note survives is then decided by live alone.
    fn judged(recorded_code: u32, live_code: Option<u32>) -> Judged<'static> {
        let live = live_code.map(answer_of);
        let record = RecordedAnswer {
            counted: Some(answer_of(recorded_code)),
            is_known_failure: true,
            note: Some("a note".to_string()),
        };
        let real = answer_of(3);
        let conformance = match &live {
            None => Conformance::Unclaimed,
            Some(counted) if *counted == real => Conformance::Agrees,
            Some(_) => Conformance::Fails,
        };
        Judged {
            case: Box::leak(Box::new(a_case("0400-a_case_built_by_a_test"))),
            outcome: Outcome::Measured(Measured {
                real,
                live,
                record: Some(Box::leak(Box::new(record))),
                exception: None,
                conformance,
                drift: None,
            }),
        }
    }

    fn answer_of(code: u32) -> Answer {
        Answer {
            counts: Counts {
                lines: 10,
                buckets: BTreeMap::from([("code".to_string(), code)]),
            },
            regions: Vec::new(),
        }
    }

    fn a_case(name: &str) -> Case {
        Case {
            name: name.to_string(),
            input_file: PathBuf::from("input.c"),
            trap: "a block".to_string(),
            truth: Truth::read(TRUTH, "/* a block\n*/ int x = 1;\n")
                .unwrap_or_else(|faults| panic!("{faults:?}")),
        }
    }
}
