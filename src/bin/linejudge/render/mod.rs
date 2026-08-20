use std::fs;
use std::path::{Path, PathBuf};

use linejudge::adapter::Adapter;
use linejudge::corpus::Corpus;
use linejudge::dialects::Dialects;

mod data;
mod measure;
mod page;

pub const DATA_FILE: &str = "data.json";
pub const INDEX_FILE: &str = "index.html";

/// Measures the whole roster and writes what a static host serves: the scoreboard, and the
/// measurement behind it as JSON beside it.
pub fn write_the_site(
    adapters: &[Adapter],
    corpus: &Corpus,
    dialects: &Dialects,
    recorded: &[PathBuf],
    find_binary: &dyn Fn(&str) -> Option<PathBuf>,
    out: &Path,
) -> Result<(), String> {
    let sweep = measure::measure_every_counter(adapters, corpus, dialects, recorded, find_binary)?;
    fs::create_dir_all(out)
        .map_err(|error| format!("{} could not be created: {error}", out.display()))?;
    let write = |name: &str, text: String| {
        fs::write(out.join(name), text)
            .map_err(|error| format!("{} could not be written: {error}", out.join(name).display()))
    };
    write(INDEX_FILE, page::render_the_scoreboard(&sweep))?;
    let json = serde_json::to_string_pretty(&sweep)
        .map_err(|error| format!("the measurement could not be written as JSON: {error}"))?;
    write(DATA_FILE, json + "\n")
}
