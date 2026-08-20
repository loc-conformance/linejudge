use std::fs;
use std::path::{Path, PathBuf};

use linejudge::adapter::Adapter;
use linejudge::corpus::Corpus;
use linejudge::dialects::Dialects;
use maud::{DOCTYPE, Markup, html};

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

/// Measures the whole roster and writes what a static host serves: the scoreboard, a page per
/// case under it, and the measurement behind them as JSON.
pub fn write_the_site(
    adapters: &[Adapter],
    corpus: &Corpus,
    dialects: &Dialects,
    recorded: &[PathBuf],
    find_binary: &dyn Fn(&str) -> Option<PathBuf>,
    out: &Path,
) -> Result<usize, String> {
    let sweep = measure::measure_every_counter(adapters, corpus, dialects, recorded, find_binary)?;
    let detailed = measure::read_every_case(&sweep, corpus, dialects)?;
    fs::create_dir_all(out)
        .map_err(|error| format!("{} could not be created: {error}", out.display()))?;
    let write = |name: &str, text: String| {
        fs::write(out.join(name), text)
            .map_err(|error| format!("{} could not be written: {error}", out.join(name).display()))
    };
    write(STYLE_FILE, STYLE.to_string())?;
    write(SCRIPT_FILE, SCRIPT.to_string())?;
    write(INDEX_FILE, scoreboard::render_the_scoreboard(&sweep))?;
    write_a_page_each(&out.join(CASES_DIR), &detailed, |detail| {
        (format!("{}.html", detail.name), case::render_one_case(detail, &sweep))
    })?;
    let tools = measure::read_every_tool(&sweep, adapters, dialects)?;
    write_a_page_each(&out.join(TOOLS_DIR), &tools, |detail| {
        (format!("{}.html", detail.name), tool::render_one_tool(detail, &sweep))
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
    let json = serde_json::to_string_pretty(&sweep)
        .map_err(|error| format!("the measurement could not be written as JSON: {error}"))?;
    write(DATA_FILE, json + "\n")?;
    Ok(detailed.len())
}

/// One directory of files, named by whatever the caller says each of them is called. Names carry
/// their own extension, since a badge is an SVG and everything else is a page.
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

/// A badge names the way of counting as well as the counter, since a counter with two of them
/// has two answers and a single file could only ever be one of them.
pub fn name_the_badge_of(counter: &str, dialect: &str) -> String {
    format!("{counter}.{dialect}")
}

/// Every page of the site is this, so the stylesheet and the script are written once and named by
/// each page rather than carried inside all of them. `up` is what a page has to climb to reach
/// the root of the site, which is nothing for the scoreboard and one step for a case.
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

/// GitHub's own mark, drawn rather than fetched, since the pages ask nothing of any other host.
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

/// Runs of whitespace in a case's own words become single spaces, since a trap and a note are
/// written across several lines in their files and read as one sentence here.
fn format_as_one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn format_the_group_title(name: &str) -> String {
    match name.split_once('-') {
        Some((number, words)) => format!("{number} · {}", words.replace('_', " ")),
        None => name.to_string(),
    }
}
