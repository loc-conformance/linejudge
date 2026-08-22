use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use linejudge::adapter::{Acquisition, Adapter};
use serde::{Deserialize, Serialize};

use crate::fetch::{CRATES_IO, GITHUB_API, GITHUB_RELEASE_ASSET, read_a_url};
use crate::style;

const ACQUISITION_BLOCK: &str = "[acquisition]";
const CRATES_IO_API: &str = "https://crates.io/api/v1/crates";
const VERSION_KEY: &str = "version";

// Only the declared version line is rewritten, so the hand alignment of the rest of the file
// survives. With `as_json` the whole of stdout is one document, so every human line goes to the
// error output instead and none of them can land inside it.
pub fn bump_every_version(
    out: &mut dyn Write,
    adapters: &[&Adapter],
    dir: &Path,
    as_json: bool,
) -> io::Result<bool> {
    let mut anything_failed = false;
    let mut moved: Vec<Moved> = Vec::new();
    for adapter in adapters {
        let name_of_counter = &adapter.name_of_counter;
        let mut say = |line: String| -> io::Result<()> {
            if as_json {
                eprintln!("{name_of_counter}: {line}");
                return Ok(());
            }
            writeln!(out, "\n{}\n  {line}", style::HEADING.paint(name_of_counter))
        };
        let Some(how) = &adapter.acquisition else {
            say(style::DETAIL
                .paint("no channel to ask, its adapter does not say where it comes from")
                .to_string())?;
            continue;
        };
        let newest = match ask_the_channel_for_the_newest(how) {
            Ok(newest) => newest,
            Err(refused) => {
                anything_failed = true;
                say(style::DIFFERS.paint(&refused).to_string())?;
                continue;
            }
        };
        if newest == how.version {
            say(format!("{} {}", style::AGREES.paint(&how.version),
                    style::DETAIL.paint("declared, nothing newer")))?;
            continue;
        }
        match write_the_version_into(dir, name_of_counter, &newest) {
            Err(refused) => {
                anything_failed = true;
                say(style::DIFFERS.paint(&refused).to_string())?;
            }
            Ok(file) => {
                say(format!("{} {} {} {}", style::RECORDED.paint(&how.version),
                        style::DETAIL.paint("raised to"), style::AGREES.paint(&newest),
                        style::DETAIL.paint(&format!("in {}", file.display()))))?;
                moved.push(Moved {
                    counter: name_of_counter.clone(),
                    from: how.version.clone(),
                    to: newest,
                });
            }
        }
    }
    // Said once at the end rather than under each counter, where several raises would repeat it.
    if !moved.is_empty() && !as_json {
        let names: Vec<&str> = moved.iter().map(|one| one.counter.as_str()).collect();
        writeln!(out, "\n{}", style::RECORDED.paint(&format!(
                "the answers recorded for {} still come from the old build, so nothing that \
                 changed since is judged until they are measured again:", names.join(", "))))?;
        for name in names {
            writeln!(out, "  {}", style::DETAIL.paint(
                    &format!("linejudge fetch {name} && linejudge record --counter {name}")))?;
        }
    }
    if as_json {
        match format_as_a_matrix(&moved) {
            Ok(matrix) => writeln!(out, "{matrix}")?,
            Err(refused) => {
                anything_failed = true;
                eprintln!("{refused}");
                writeln!(out, "[]")?;
            }
        }
    }
    Ok(anything_failed)
}

fn write_the_version_into(dir: &Path, name_of_counter: &str, to: &str) -> Result<PathBuf, String> {
    let file = dir.join(format!("{name_of_counter}.toml"));
    let held = fs::read_to_string(&file)
        .map_err(|error| format!("{} could not be read: {error}", file.display()))?;
    let raised = raise_the_version_line(&held, to)
        .map_err(|refused| format!("{}: {refused}", file.display()))?;
    fs::write(&file, raised)
        .map_err(|error| format!("{} could not be written: {error}", file.display()))?;
    Ok(file)
}

fn ask_the_channel_for_the_newest(how: &Acquisition) -> Result<String, String> {
    match how.channel.as_str() {
        CRATES_IO => {
            let url = format!("{CRATES_IO_API}/{}", how.name);
            read_the_newest_from_crates_io(&read_a_url(&url)?)
        }
        GITHUB_RELEASE_ASSET => {
            let url = format!("{GITHUB_API}/{}/releases/latest", how.name);
            read_the_newest_from_a_release(&read_a_url(&url)?)
        }
        other => Err(format!(
            "{other} is not a channel this knows, and it speaks {CRATES_IO} and \
             {GITHUB_RELEASE_ASSET}"
        )),
    }
}

fn read_the_newest_from_crates_io(document: &str) -> Result<String, String> {
    let answer: CratesIoAnswer = serde_json::from_str(document)
        .map_err(|error| format!("what crates.io answered does not read: {error}"))?;
    Ok(answer.the_crate.max_stable_version)
}

// A release is named by its tag, which the same project writes as `3.7.0` or `v3.7.0` from one
// release to the next, while an adapter declares the number alone.
fn read_the_newest_from_a_release(document: &str) -> Result<String, String> {
    let release: Release = serde_json::from_str(document)
        .map_err(|error| format!("what github answered does not read: {error}"))?;
    Ok(release.tag_name.strip_prefix('v').unwrap_or(&release.tag_name).to_string())
}

// Only the value between the quotes moves. Rewriting the file through a serialiser would reflow
// every line of it, and these are aligned by hand.
fn raise_the_version_line(held: &str, to: &str) -> Result<String, String> {
    let mut inside = false;
    let mut raised = false;
    let mut lines: Vec<String> = Vec::new();
    for line in held.lines() {
        let opens_a_block = line.trim_start().starts_with('[');
        if opens_a_block {
            inside = line.trim() == ACQUISITION_BLOCK;
        }
        let names_the_version =
            line.split_once('=').is_some_and(|(key, _)| key.trim() == VERSION_KEY);
        if !inside || raised || opens_a_block || !names_the_version {
            lines.push(line.to_string());
            continue;
        }
        let opened = line.find('"').ok_or("its version is not written between quotes")?;
        let closed = line[opened + 1..]
            .find('"')
            .map(|at| at + opened + 1)
            .ok_or("its version opens a quote that nothing closes")?;
        lines.push(format!("{}{to}{}", &line[..=opened], &line[closed..]));
        raised = true;
    }
    if !raised {
        return Err(format!("it has no {VERSION_KEY} line under {ACQUISITION_BLOCK}"));
    }
    let mut written = lines.join("\n");
    if held.ends_with('\n') {
        written.push('\n');
    }
    Ok(written)
}

// A github matrix takes an array of objects, one job per entry, and every field of an entry is
// readable from the job, so the branch this raise belongs on needs no parsing anywhere.
fn format_as_a_matrix(moved: &[Moved]) -> Result<String, String> {
    serde_json::to_string(moved)
        .map_err(|error| format!("what moved could not be written as JSON: {error}"))
}

#[derive(Serialize)]
struct Moved {
    counter: String,
    from: String,
    to: String,
}

#[derive(Deserialize)]
struct CratesIoAnswer {
    #[serde(rename = "crate")]
    the_crate: CrateFacts,
}

#[derive(Deserialize)]
struct CrateFacts {
    max_stable_version: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    const AN_ADAPTER: &str = "name       = \"scc\"\n\
                              repository = \"https://github.com/boyter/scc\"\n\
                              \n\
                              [acquisition]\n\
                              channel = \"github-release-asset\"\n\
                              name    = \"boyter/scc\"\n\
                              version = \"3.7.0\"\n\
                              \n\
                              [dialect.default]\n\
                              args = []\n";

    #[test]
    fn only_the_version_between_the_quotes_moves() {
        let raised = raise_the_version_line(AN_ADAPTER, "3.8.0").unwrap();
        assert_eq!(raised, AN_ADAPTER.replace("\"3.7.0\"", "\"3.8.0\""));
        assert!(raised.ends_with("args = []\n"), "{raised}");
    }

    #[test]
    fn a_version_outside_the_acquisition_block_is_left_alone() {
        let elsewhere = AN_ADAPTER.replace("args = []", "version = \"1.0.0\"");
        let raised = raise_the_version_line(&elsewhere, "3.8.0").unwrap();
        assert!(raised.contains("version = \"3.8.0\""), "{raised}");
        assert!(raised.contains("version = \"1.0.0\""), "{raised}");
    }

    #[test]
    fn an_adapter_with_no_acquisition_block_is_refused_rather_than_written() {
        let none = "name = \"mezura\"\n\n[dialect.default]\nargs = []\n";
        let refused = raise_the_version_line(none, "3.8.0").unwrap_err();
        assert!(refused.contains("[acquisition]"), "{refused}");
    }

    #[test]
    fn the_tag_of_a_release_is_read_with_or_without_its_v() {
        let of = |tag: &str| {
            read_the_newest_from_a_release(&format!("{{\"tag_name\":\"{tag}\"}}")).unwrap()
        };
        assert_eq!(of("v3.8.0"), "3.8.0");
        assert_eq!(of("3.8.0"), "3.8.0");
    }

    #[test]
    fn crates_io_is_read_for_its_newest_stable_and_never_for_a_prerelease() {
        let document = "{\"crate\":{\"max_version\":\"15.0.0-beta.1\",\
                        \"max_stable_version\":\"14.0.0\"}}";
        assert_eq!(read_the_newest_from_crates_io(document).unwrap(), "14.0.0");
    }

    #[test]
    fn an_answer_that_does_not_read_is_a_refusal_and_never_a_panic() {
        assert!(read_the_newest_from_crates_io("not json at all").is_err());
        assert!(read_the_newest_from_a_release("{\"nothing\":1}").is_err());
    }

    #[test]
    fn the_matrix_names_every_counter_that_moved_and_both_of_its_versions() {
        let moved = [Moved {
            counter: "scc".to_string(),
            from: "3.7.0".to_string(),
            to: "3.8.0".to_string(),
        }];
        assert_eq!(
            format_as_a_matrix(&moved).unwrap(),
            "[{\"counter\":\"scc\",\"from\":\"3.7.0\",\"to\":\"3.8.0\"}]"
        );
        assert_eq!(format_as_a_matrix(&[]).unwrap(), "[]");
    }
}
