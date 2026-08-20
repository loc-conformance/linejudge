use std::fs;
use std::path::{Path, PathBuf};

use linejudge::adapter::Adapter;
use linejudge::corpus::Corpus;
use linejudge::dialects::Dialects;
use maud::{DOCTYPE, Markup, html};

mod case;
mod data;
mod measure;
mod scoreboard;

pub const DATA_FILE: &str = "data.json";
pub const INDEX_FILE: &str = "index.html";
pub const CASES_DIR: &str = "cases";
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
    let cases = out.join(CASES_DIR);
    fs::create_dir_all(&cases)
        .map_err(|error| format!("{} could not be created: {error}", cases.display()))?;
    for detail in &detailed {
        let page = cases.join(format!("{}.html", detail.name));
        fs::write(&page, case::render_one_case(detail, &sweep))
            .map_err(|error| format!("{} could not be written: {error}", page.display()))?;
    }
    let json = serde_json::to_string_pretty(&sweep)
        .map_err(|error| format!("the measurement could not be written as JSON: {error}"))?;
    write(DATA_FILE, json + "\n")?;
    Ok(detailed.len())
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
