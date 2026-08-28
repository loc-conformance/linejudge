use std::fs;
use std::path::{Path, PathBuf};

use linejudge::adapter::Adapter;
use linejudge::corpus::Corpus;
use linejudge::dialects::Dialects;
use maud::{DOCTYPE, Markup, html};

use crate::render::data::{Answer, CaseDetail, Sweep, ToolDetail, Verdict};

pub mod badge;
mod case;
mod data;
mod measure;
mod overview;
mod tool;

pub const DATA_FILE: &str = "data.json";
pub const INDEX_FILE: &str = "index.html";
pub const BADGES_DIR: &str = "badges";
pub const CASES_DIR: &str = "cases";
pub const TOOLS_DIR: &str = "tools";
const ICON_FILE: &str = "favicon.svg";
const SCRIPT_FILE: &str = "page.js";
const STYLE_FILE: &str = "page.css";
const STYLE: &str = include_str!("page.css");
const SCRIPT: &str = include_str!("page.js");
const MARK_WEDGE: &str = "M32 41 L46 55 L18 55 Z";
const MARK_LINES: &str = "#0969da";
const MARK_FRAME_ON_PAPER: &str = "#303a4a";
const MARK_FRAME_ON_INK: &str = "#c3ccd8";

pub fn write_the_site(
    adapters: &[Adapter],
    corpus: &Corpus,
    dialects: &Dialects,
    recorded: &[PathBuf],
    find_binary: &dyn Fn(&str) -> Option<PathBuf>,
    out: &Path,
    every_input_is_ours: bool,
) -> Result<usize, String> {
    let sweep = measure::measure_every_counter(adapters, corpus, dialects, recorded, find_binary)?;
    let cases = measure::read_every_case(&sweep, corpus, dialects)?;
    let tools = measure::read_every_tool(&sweep, adapters, dialects)?;
    write_every_file(&sweep, &cases, &tools, out, every_input_is_ours)
}

// Kept apart from the measuring, so what the pages point at can be checked with no counter on the
// machine.
fn write_every_file(
    sweep: &Sweep,
    cases: &[CaseDetail],
    tools: &[ToolDetail],
    out: &Path,
    every_input_is_ours: bool,
) -> Result<usize, String> {
    fs::create_dir_all(out)
        .map_err(|error| format!("{} could not be created: {error}", out.display()))?;
    let write = |name: &str, text: String| {
        fs::write(out.join(name), text)
            .map_err(|error| format!("{} could not be written: {error}", out.join(name).display()))
    };
    write(STYLE_FILE, STYLE.to_string())?;
    write(SCRIPT_FILE, SCRIPT.to_string())?;
    write(ICON_FILE, build_the_icon())?;
    write(INDEX_FILE, overview::render_the_overview(sweep))?;
    write_a_page_each(&out.join(CASES_DIR), cases, |detail| {
        (format!("{}.html", detail.name), case::render_one_case(detail, sweep))
    })?;
    write_a_page_each(&out.join(TOOLS_DIR), tools, |detail| {
        (format!("{}.html", detail.name), tool::render_one_tool(detail, sweep))
    })?;
    // A badge is read as this suite's verdict wherever it is embedded, so over cases or rules of
    // somebody's own it would be a verdict nobody can check. The pages still go out, since looking
    // at your own corpus locally is what those flags are for.
    let badges_dir = out.join(BADGES_DIR);
    if every_input_is_ours {
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
        write_a_page_each(&badges_dir, &badges, |(name, svg)| {
            (format!("{name}.svg"), svg.clone())
        })?;
    } else if badges_dir.exists() {
        // Nothing empties this directory, so not writing a badge would leave the one an earlier
        // run put here, and the site would go out with a verdict on cases it no longer holds.
        fs::remove_dir_all(&badges_dir)
            .map_err(|error| format!("{} could not be removed: {error}", badges_dir.display()))?;
    }
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

// How many answers are in each of the five states. The badge and the overview both show these
// and both read them from here, in this order, so neither can come to mean something else by
// "open".
pub struct StateCounts {
    pub agrees: usize,
    pub open: usize,
    pub fails: usize,
    pub unclaimed: usize,
    pub broke: usize,
}

impl StateCounts {
    pub fn of(answers: &[Answer]) -> StateCounts {
        let of = |wanted: &dyn Fn(&Answer) -> bool| answers.iter().filter(|a| wanted(a)).count();
        StateCounts {
            agrees: of(&|a| a.verdict == Verdict::Agrees),
            open: of(&|a| a.verdict == Verdict::Fails && a.note.is_none()),
            fails: of(&|a| a.verdict == Verdict::Fails && a.note.is_some()),
            unclaimed: of(&|a| a.verdict == Verdict::Unclaimed),
            broke: of(&|a| a.verdict == Verdict::Broke),
        }
    }
}

// Every page is this, so the stylesheet and the script are written once and linked rather than
// carried inside each one. `up` is what a page climbs to reach the root of the site: nothing for
// the overview, one step for a case.
fn wrap_the_page(title: &str, body: Markup, up: &str) -> String {
    let page = html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) }
                link rel="icon" type="image/svg+xml" href=(format!("{up}{ICON_FILE}"));
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

// The scales, drawn once and used twice: inline in the header, where the frame takes the color of
// the text around it, and inside the icon file, where a rule of its own supplies that color.
pub fn render_the_mark_of_linejudge() -> Markup {
    html! {
        g fill="currentColor" {
            rect x="3" y="11" width="58" height="7" rx="3.5" {}
            rect x="29" y="8" width="6" height="35" rx="3" {}
            rect x="10.5" y="18" width="4" height="7" {}
            rect x="48.5" y="18" width="4" height="7" {}
            path d=(MARK_WEDGE) stroke="currentColor" stroke-width="4" stroke-linejoin="round" {}
        }
        g fill=(MARK_LINES) {
            rect x="1" y="25" width="23" height="8" rx="4" {}
            rect x="4" y="37" width="17" height="8" rx="4" {}
            rect x="39" y="25" width="23" height="8" rx="4" {}
        }
    }
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

// An icon file stands on its own with no page around it to take a color from, and a tab bar can be
// either light or dark, so the rule it carries is the only thing keeping the frame visible in both.
fn build_the_icon() -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 64 64\">\
         <style>svg{{color:{MARK_FRAME_ON_PAPER}}}\
         @media(prefers-color-scheme:dark){{svg{{color:{MARK_FRAME_ON_INK}}}}}</style>\
         {}</svg>\n",
        render_the_mark_of_linejudge().into_string()
    )
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

    // A name built by one function and a directory made by another are held together by nothing
    // else.
    #[test]
    fn every_page_points_only_at_files_the_site_actually_holds() {
        let out = env::temp_dir().join("linejudge-every_page_points_at_what_is_there");
        let _ = fs::remove_dir_all(&out);
        let sweep = a_sweep();
        let cases = [a_case("1010-a_case", "1000-comments"), a_case("2010-another_case", "2000-strings")];
        let tools = [a_tool("mezura", &["content", "region"]), a_tool("tokei", &["default"])];

        let written = write_every_file(&sweep, &cases, &tools, &out, true).unwrap();

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

    // It writes twice on purpose: the first run is what leaves a badge for the second to find.
    #[test]
    fn inputs_of_somebody_s_own_get_the_whole_site_and_leave_no_badge_behind() {
        let out = env::temp_dir().join("linejudge-a_foreign_input_earns_no_badge");
        let _ = fs::remove_dir_all(&out);
        let sweep = a_sweep();
        let cases = [a_case("1010-a_case", "1000-comments")];
        let tools = [a_tool("mezura", &["content", "region"])];

        write_every_file(&sweep, &cases, &tools, &out, true).unwrap();
        let badges_first = out.join(BADGES_DIR).join("mezura.region.svg").is_file();
        write_every_file(&sweep, &cases, &tools, &out, false).unwrap();

        let badges = out.join(BADGES_DIR).exists();
        let overview = out.join(INDEX_FILE).is_file();
        fs::remove_dir_all(&out).unwrap();

        assert!(badges_first, "the run over our own inputs writes them");
        assert!(!badges, "a badge nobody can check is taken away and not merely left unwritten");
        assert!(overview, "the pages are still written, which is what a local look at them is for");
    }

    // The site's icon is generated and the file GitHub and the README are pointed at is committed,
    // so the two are one drawing kept in two places and nothing else holds them together.
    #[test]
    fn the_committed_mark_is_the_icon_the_site_is_given() {
        let kept = fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/logo").join("linejudge.svg"),
        )
        .unwrap_or_else(|error| panic!("assets/logo/linejudge.svg: {error}"))
        .replace("\r\n", "\n");
        assert_eq!(
            kept.trim_end(),
            build_the_icon().trim_end(),
            "regenerate assets/logo/linejudge.svg from the icon the render writes",
        );
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
            linejudge: "0.1.0".to_string(),
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
