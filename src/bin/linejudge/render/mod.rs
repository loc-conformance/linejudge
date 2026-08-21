use std::fs;
use std::path::{Path, PathBuf};

use linejudge::adapter::Adapter;
use linejudge::corpus::Corpus;
use linejudge::dialects::Dialects;
use maud::{DOCTYPE, Markup, html};

use crate::render::data::{CaseDetail, Sweep, ToolDetail};

mod badge;
mod case;
mod data;
mod measure;
mod scoreboard;
mod tool;

pub const DATA_FILE: &str = "data.json";
pub const INDEX_FILE: &str = "index.html";
pub const BADGES_DIR: &str = "badges";
pub const CASES_DIR: &str = "cases";
pub const TOOLS_DIR: &str = "tools";
const SCRIPT_FILE: &str = "page.js";
const STYLE_FILE: &str = "page.css";
const STYLE: &str = include_str!("page.css");
const SCRIPT: &str = include_str!("page.js");

// Measures the whole roster and writes what a static host serves: the scoreboard, a page per case
// under it, and the measurement behind them as JSON.
pub fn write_the_site(
    adapters: &[Adapter],
    corpus: &Corpus,
    dialects: &Dialects,
    recorded: &[PathBuf],
    find_binary: &dyn Fn(&str) -> Option<PathBuf>,
    out: &Path,
) -> Result<usize, String> {
    let sweep = measure::measure_every_counter(adapters, corpus, dialects, recorded, find_binary)?;
    let cases = measure::read_every_case(&sweep, corpus, dialects)?;
    let tools = measure::read_every_tool(&sweep, adapters, dialects)?;
    write_every_file(&sweep, &cases, &tools, out)
}

// Kept apart from the measuring, so what the pages point at can be checked with no counter on the
// machine.
fn write_every_file(
    sweep: &Sweep,
    cases: &[CaseDetail],
    tools: &[ToolDetail],
    out: &Path,
) -> Result<usize, String> {
    fs::create_dir_all(out)
        .map_err(|error| format!("{} could not be created: {error}", out.display()))?;
    let write = |name: &str, text: String| {
        fs::write(out.join(name), text)
            .map_err(|error| format!("{} could not be written: {error}", out.join(name).display()))
    };
    write(STYLE_FILE, STYLE.to_string())?;
    write(SCRIPT_FILE, SCRIPT.to_string())?;
    write(INDEX_FILE, scoreboard::render_the_scoreboard(sweep))?;
    write_a_page_each(&out.join(CASES_DIR), cases, |detail| {
        (format!("{}.html", detail.name), case::render_one_case(detail, sweep))
    })?;
    write_a_page_each(&out.join(TOOLS_DIR), tools, |detail| {
        (format!("{}.html", detail.name), tool::render_one_tool(detail, sweep))
    })?;
    let badges: Vec<(String, String)> = sweep
        .counters
        .iter()
        .flat_map(|counter| {
            counter.dialects.iter().map(|dialect| {
                (name_the_badge_of(&counter.name, &dialect.name),
                 badge::render_one_badge(&dialect.answers))
            })
        })
        .collect();
    write_a_page_each(&out.join(BADGES_DIR), &badges, |(name, svg)| {
        (format!("{name}.svg"), svg.clone())
    })?;
    let json = serde_json::to_string_pretty(sweep)
        .map_err(|error| format!("the measurement could not be written as JSON: {error}"))?;
    write(DATA_FILE, json + "\n")?;
    Ok(cases.len())
}

// The names carry their own extension, since a badge is an SVG and everything else is a page.
fn write_a_page_each<T>(
    dir: &Path,
    each: &[T],
    render: impl Fn(&T) -> (String, String),
) -> Result<(), String> {
    fs::create_dir_all(dir)
        .map_err(|error| format!("{} could not be created: {error}", dir.display()))?;
    for one in each {
        let (name, text) = render(one);
        let page = dir.join(&name);
        fs::write(&page, text)
            .map_err(|error| format!("{} could not be written: {error}", page.display()))?;
    }
    Ok(())
}

// A badge names the way of counting as well as the counter, since a counter with two of them has
// two answers and one file could only ever be one of them.
pub fn name_the_badge_of(name_of_counter: &str, name_of_dialect: &str) -> String {
    format!("{name_of_counter}.{name_of_dialect}")
}

// Every page is this, so the stylesheet and the script are written once and linked rather than
// carried inside each one. `up` is what a page climbs to reach the root of the site: nothing for
// the scoreboard, one step for a case.
fn wrap_the_page(title: &str, body: Markup, up: &str) -> String {
    let page = html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) }
                link rel="stylesheet" href=(format!("{up}{STYLE_FILE}"));
            }
            body {
                div .wrap { (body) }
                script src=(format!("{up}{SCRIPT_FILE}")) {}
            }
        }
    };
    page.into_string()
}

// GitHub's own mark, drawn rather than fetched, since the pages ask nothing of any other host.
const GITHUB_MARK: &str = "M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 \
    0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01\
    -.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89\
    -3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 \
    2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 \
    2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8\
    .012 8.012 0 0 0 16 8c0-4.42-3.58-8-8-8z";

pub fn render_the_mark_of_github() -> Markup {
    html! {
        svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true" {
            path d=(GITHUB_MARK) fill="currentColor" {}
        }
    }
}

// A trap and a note are written across several lines in their files and read as one sentence here.
fn format_as_one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn format_the_group_title(name: &str) -> String {
    match name.split_once('-') {
        Some((number, words)) => format!("{number} · {}", words.replace('_', " ")),
        None => name.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::env;

    use linejudge::truth::Covering;

    use crate::render::data::{
        Answer, Case, Counted, Counter, Counts, Dialect, DialectDetail, Group, Line, Piece,
        RuleDetail, Verdict,
    };

    use super::*;

    // Each page is rendered by a function that knows nothing of where the others were written, so
    // a name built in one place and a directory made in another are held together by nothing but
    // this.
    #[test]
    fn every_page_points_only_at_files_the_site_actually_holds() {
        let out = env::temp_dir().join("linejudge-every_page_points_at_what_is_there");
        let _ = fs::remove_dir_all(&out);
        let sweep = a_sweep();
        let cases = [a_case("1010-a_case", "1000-comments"), a_case("2010-another_case", "2000-strings")];
        let tools = [a_tool("mezura", &["content", "region"]), a_tool("tokei", &["default"])];

        let written = write_every_file(&sweep, &cases, &tools, &out).unwrap();

        let mut missing = Vec::new();
        for page in find_every_page_under(&out) {
            let here = page.parent().unwrap_or(&out).to_path_buf();
            let text = fs::read_to_string(&page).unwrap();
            for link in find_every_link_in(&text) {
                if link.starts_with("http") || link.starts_with('#') {
                    continue;
                }
                if !here.join(&link).exists() {
                    missing.push(format!("{} points at {link}", page.display()));
                }
            }
        }
        let read_back: Sweep = serde_json::from_str(
            &fs::read_to_string(out.join(DATA_FILE)).unwrap()).unwrap();
        let badge = out.join(BADGES_DIR).join("mezura.region.svg").is_file();
        fs::remove_dir_all(&out).unwrap();

        assert!(missing.is_empty(), "{}", missing.join("\n"));
        assert_eq!(written, 2);
        assert_eq!(read_back, sweep, "what data.json holds is the measurement itself");
        assert!(badge, "one badge per counter and way of counting");
    }

    fn find_every_page_under(dir: &Path) -> Vec<PathBuf> {
        let mut found = Vec::new();
        for entry in fs::read_dir(dir).into_iter().flatten().filter_map(|entry| entry.ok()) {
            let path = entry.path();
            match path.is_dir() {
                true => found.extend(find_every_page_under(&path)),
                false if path.extension().is_some_and(|end| end == "html") => found.push(path),
                false => {}
            }
        }
        found
    }

    fn find_every_link_in(page: &str) -> Vec<String> {
        let mut found = Vec::new();
        for opener in ["href=\"", "src=\""] {
            let mut rest = page;
            while let Some(at) = rest.find(opener) {
                rest = &rest[at + opener.len()..];
                let Some(end) = rest.find('"') else { break };
                found.push(rest[..end].to_string());
                rest = &rest[end..];
            }
        }
        found
    }

    fn a_sweep() -> Sweep {
        let answer = |name_of_case: &str, verdict: Verdict| {
            let counts = |code: u32| Counts {
                lines: 3,
                buckets: BTreeMap::from([("code".to_string(), code)]),
            };
            Answer {
                case: name_of_case.to_string(),
                verdict,
                wants: (verdict != Verdict::Broke).then(|| counts(3)),
                answered: matches!(verdict, Verdict::Agrees | Verdict::Fails).then(|| counts(2)),
                wants_regions: Vec::new(),
                answered_regions: Vec::new(),
                note: (verdict == Verdict::Fails).then(|| "the /* opens a comment".to_string()),
                exception: None,
                broke: (verdict == Verdict::Broke).then(|| "it exited 2".to_string()),
                command: format!("tokei cases/{name_of_case}/input.c"),
            }
        };
        let way = |name: &str, first: Verdict, second: Verdict| Dialect {
            name: name.to_string(),
            answers: vec![answer("1010-a_case", first), answer("2010-another_case", second)],
        };
        Sweep {
            measured_on: "2026-08-20".to_string(),
            groups: vec![
                Group {
                    name: "1000-comments".to_string(),
                    cases: vec![
                        Case {
                            name: "1010-a_case".to_string(),
                            trap: "a trap".to_string(),
                            disabled: false,
                        },
                        // Set aside, so the scoreboard names it and links it nowhere.
                        Case {
                            name: "disabled-1020-set_aside".to_string(),
                            trap: String::new(),
                            disabled: true,
                        },
                    ],
                },
                Group {
                    name: "2000-strings".to_string(),
                    cases: vec![Case {
                        name: "2010-another_case".to_string(),
                        trap: "another trap".to_string(),
                        disabled: false,
                    }],
                },
            ],
            counters: vec![
                Counter {
                    name: "mezura".to_string(),
                    version: "v3.0.0".to_string(),
                    dialects: vec![
                        way("content", Verdict::Agrees, Verdict::Fails),
                        way("region", Verdict::Broke, Verdict::Agrees),
                    ],
                },
                Counter {
                    name: "tokei".to_string(),
                    version: "tokei 14.0.0".to_string(),
                    dialects: vec![way("default", Verdict::Unclaimed, Verdict::Fails)],
                },
            ],
        }
    }

    fn a_case(name: &str, group: &str) -> CaseDetail {
        CaseDetail {
            name: name.to_string(),
            group: group.to_string(),
            trap: "a trap".to_string(),
            file: "input.c".to_string(),
            ways: ["mezura.content", "mezura.region", "tokei.default"]
                .map(str::to_string)
                .to_vec(),
            lines: vec![Line {
                pieces: vec![
                    Piece { covering: Covering::Residue, text: "a = 1; ".to_string() },
                    Piece { covering: Covering::Comment, text: "// two".to_string() },
                ],
                counted: ["code", "comments", "code"]
                    .iter()
                    .map(|bucket| Counted {
                        bucket: bucket.to_string(),
                        rules: vec!["a-rule".to_string()],
                        region: None,
                    })
                    .collect(),
            }],
        }
    }

    fn a_tool(name: &str, ways: &[&str]) -> ToolDetail {
        ToolDetail {
            name: name.to_string(),
            version: format!("{name} 1.0.0"),
            repository: Some(format!("https://github.com/nobody/{name}")),
            channel: Some("crates-io".to_string()),
            dialects: ways
                .iter()
                .map(|way| DialectDetail {
                    name: way.to_string(),
                    flags: vec!["--mode".to_string(), way.to_string()],
                    rules: vec![RuleDetail {
                        name: "a-comment-alone-is-comments".to_string(),
                        bucket: "comments".to_string(),
                        when: vec!["part of the line is inside a comment".to_string()],
                    }],
                })
                .collect(),
        }
    }
}
