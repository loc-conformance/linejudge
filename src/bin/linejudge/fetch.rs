use std::env::consts::{ARCH, EXE_SUFFIX, OS};
use std::fs;
use std::io::{self, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use linejudge::adapter::{Acquisition, Adapter, is_the_declared_version};
use linejudge::adapter::{
    CHANNELS, CRATES_IO, GITHUB_RELEASE_ASSET, GITHUB_RELEASE_FILE, OTHER_SYSTEM,
    VERSION_PLACEHOLDER,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::counters::Counters;
use crate::fetched::{create_a_partial_dir_for, find_the_binary_of, finish_the_partial_dir};
use crate::style;

pub(crate) const GITHUB_API: &str = "https://api.github.com/repos";

const CHECKSUM_WORDS: [&str; 2] = ["checksum", "sha256"];
// crates.io answers 403 to a request that arrives as plain curl, which reads exactly like a crate
// that is not there, so every request says who it is and where to complain about it.
const USER_AGENT: &str =
    concat!("linejudge/", env!("CARGO_PKG_VERSION"), " (+", env!("CARGO_PKG_REPOSITORY"), ")");

// Downloads every counter that says where it comes from, at exactly the version it declares. One
// counter failing never stops the others, so whether anything went wrong is answered at the end.
pub fn fetch_every_counter(
    out: &mut dyn Write,
    adapters: &[&Adapter],
    named: &Counters,
) -> io::Result<bool> {
    let mut anything_failed = false;
    for adapter in adapters {
        let name_of_counter = &adapter.name_of_counter;
        writeln!(out, "\n{}", style::HEADING.paint(name_of_counter))?;
        let Some(how) = &adapter.acquisition else {
            writeln!(out, "  {}", style::DETAIL.paint(
                    "its adapter does not say where to download it from, so it is skipped"))?;
            continue;
        };
        let arrived = match find_the_binary_of(name_of_counter, &how.version) {
            Some(there) => Ok((there, "is already here")),
            None => fetch_one_counter(out, adapter, how).map(|binary| (binary, "is ready")),
        };
        match arrived {
            Ok((binary, standing)) => {
                writeln!(out, "  {} {}",
                        style::AGREES.paint(&format!("{} {standing}", how.version)),
                        style::DETAIL.paint(&binary.display().to_string()))?;
                report_the_path_that_shadows(out, name_of_counter, named)?;
            }
            Err(refused) => {
                anything_failed = true;
                writeln!(out, "  {}", style::DIFFERS.paint(&refused))?;
            }
        }
    }
    Ok(anything_failed)
}

// A path in `counters.toml` beats a download, since it is somebody saying which binary they mean.
// Said out loud, or a fetch looks like it changed what gets measured when it did not.
fn report_the_path_that_shadows(
    out: &mut dyn Write,
    name_of_counter: &str,
    named: &Counters,
) -> io::Result<()> {
    let Some(instead) = named.find_binary(name_of_counter) else { return Ok(()) };
    writeln!(out, "  {}", style::RECORDED.paint(&format!(
            "counters.toml names {}, so that is what a run measures and not this",
            instead.display())))
}

fn fetch_one_counter(
    out: &mut dyn Write,
    adapter: &Adapter,
    how: &Acquisition,
) -> Result<PathBuf, String> {
    let name_of_counter = &adapter.name_of_counter;
    let partial = create_a_partial_dir_for(name_of_counter, &how.version)?;
    let assembled = match how.channel.as_str() {
        CRATES_IO => build_from_crates_io(out, name_of_counter, how, &partial),
        GITHUB_RELEASE_ASSET => download_a_release_asset(out, name_of_counter, how, &partial),
        GITHUB_RELEASE_FILE => download_a_named_release_file(out, name_of_counter, how, &partial),
        other => Err(format!(
            "{other} is not a channel this program knows: it knows {}",
            CHANNELS.join(", ")
        )),
    };
    let binary = assembled.inspect_err(|_| {
        let _ = fs::remove_dir_all(&partial);
    })?;
    let printed = adapter.read_version(&binary).map_err(|refused| {
        let _ = fs::remove_dir_all(&partial);
        format!("{name_of_counter} arrived and cannot answer its version flag on this machine: {refused}")
    })?;
    if !is_the_declared_version(&how.version, &printed) {
        let _ = fs::remove_dir_all(&partial);
        return Err(format!(
            "{name_of_counter} {} was asked for and what arrived says it is \"{printed}\", so the \
             channel handed over something else",
            how.version
        ));
    }
    let dir = finish_the_partial_dir(&partial)?;
    Ok(dir.join(format!("{name_of_counter}{EXE_SUFFIX}")))
}

// crates.io holds source and never an executable, so this channel is a compile and needs cargo on
// the machine. Cargo installs into `<root>/bin`, one level deeper than the lookup goes, so the
// binary is lifted out of there and the rest of what cargo wrote is left behind.
fn build_from_crates_io(
    out: &mut dyn Write,
    name_of_counter: &str,
    how: &Acquisition,
    into: &Path,
) -> Result<PathBuf, String> {
    let root = into.join("cargo");
    say(out, &format!("building {} {} from crates.io, which takes a while", how.name, how.version))?;
    let installing =
        ["install", &how.name, "--version", &how.version, "--root", &root.display().to_string()]
            .map(str::to_string);
    let asked: Vec<&str> = installing.iter().map(String::as_str).collect();
    run_the_program("cargo", &asked).map_err(Refusal::into_words)?;
    let named = format!("{name_of_counter}{EXE_SUFFIX}");
    let built = root.join("bin").join(&named);
    if !built.is_file() {
        return Err(format!(
            "cargo built {} {} and no {named} came out of it, so the crate installs its command \
             under another name",
            how.name, how.version
        ));
    }
    let binary = into.join(&named);
    fs::rename(&built, &binary)
        .map_err(|error| format!("{} could not be moved into place: {error}", built.display()))?;
    Ok(binary)
}

// The release is asked for by tag, the file for this machine picked out of what it carries, and
// the checksums published beside it say whether the download arrived whole.
fn download_a_release_asset(
    out: &mut dyn Write,
    name_of_counter: &str,
    how: &Acquisition,
    into: &Path,
) -> Result<PathBuf, String> {
    let release = find_the_release_of(&how.name, &how.version)?;
    let asset = find_the_asset_for_this_machine(&release.assets)?;
    let downloaded = name_the_file_of(asset)?;
    say(out, &format!("downloading {downloaded}"))?;
    let archive = into.join(&downloaded);
    save_a_url(&asset.browser_download_url, &archive)?;
    check_it_arrived_whole(&release.assets, &downloaded, &archive)?;
    let unpacking =
        ["-xf", &archive.display().to_string(), "-C", &into.display().to_string()].map(str::to_string);
    let asked: Vec<&str> = unpacking.iter().map(String::as_str).collect();
    run_the_program("tar", &asked).map_err(|refused| {
        format!("{downloaded} could not be unpacked: {}", refused.into_words())
    })?;
    let _ = fs::remove_file(&archive);
    let named = format!("{name_of_counter}{EXE_SUFFIX}");
    find_the_file_named(&named, into).ok_or_else(|| {
        format!("{downloaded} holds no {named}, so this counter is packed under another name")
    })
}

// The release publishes plain files whose names say nothing about systems, so the adapter names
// the one to take for each system outright. Nothing is unpacked, so it is saved straight under
// the name the lookup goes by. A script among them runs off its own first line, which names its
// interpreter, once it is marked runnable.
fn download_a_named_release_file(
    out: &mut dyn Write,
    name_of_counter: &str,
    how: &Acquisition,
    into: &Path,
) -> Result<PathBuf, String> {
    let wanted = choose_the_file_for(how, OS)?;
    let release = find_the_release_of(&how.name, &how.version)?;
    let asset = release.assets.iter().find(|asset| asset.name == wanted).ok_or_else(|| {
        format!(
            "this release carries no {wanted}, only {}",
            release.assets.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
        )
    })?;
    say(out, &format!("downloading {wanted}"))?;
    let binary = into.join(format!("{name_of_counter}{EXE_SUFFIX}"));
    save_a_url(&asset.browser_download_url, &binary)?;
    check_it_arrived_whole(&release.assets, &wanted, &binary)?;
    mark_it_runnable(&binary)?;
    Ok(binary)
}

fn choose_the_file_for(how: &Acquisition, os: &str) -> Result<String, String> {
    let files = how.file.as_ref().ok_or_else(|| {
        format!("{GITHUB_RELEASE_FILE} needs an [acquisition.file] table naming the file to take")
    })?;
    let named = files.get(os).or_else(|| files.get(OTHER_SYSTEM)).ok_or_else(|| {
        format!("its adapter names no release file for {os} and none for {OTHER_SYSTEM}")
    })?;
    Ok(named.replace(VERSION_PLACEHOLDER, &how.version))
}

// What arrives is a plain file, and unix runs only what is marked runnable. Windows decides by
// extension and needs nothing.
#[cfg(unix)]
fn mark_it_runnable(binary: &Path) -> Result<(), String> {
    let mut runnable = fs::metadata(binary)
        .map_err(|error| format!("{} could not be read: {error}", binary.display()))?
        .permissions();
    runnable.set_mode(runnable.mode() | 0o755);
    fs::set_permissions(binary, runnable)
        .map_err(|error| format!("{} could not be marked runnable: {error}", binary.display()))
}

#[cfg(not(unix))]
fn mark_it_runnable(_binary: &Path) -> Result<(), String> {
    Ok(())
}

// The name is kept as the release gives it, since that is what the published checksums name and
// what any message has to say. It came off the network, so a name carrying a directory inside it
// is refused: it would write outside the folder being assembled.
fn name_the_file_of(asset: &Asset) -> Result<String, String> {
    let plain = Path::new(&asset.name).file_name().and_then(|name| name.to_str());
    match plain == Some(asset.name.as_str()) {
        true => Ok(asset.name.clone()),
        false => Err(format!("{} is a file name with a path inside it", asset.name)),
    }
}

// Every project names this file differently, `checksums.txt` here and `SHA256SUMS` there, so it is
// recognised by the word in it. A release that publishes none is downloaded without the check.
fn find_the_checksums_among(assets: &[Asset]) -> Option<&Asset> {
    assets.iter().find(|asset| {
        let name = asset.name.to_lowercase();
        CHECKSUM_WORDS.iter().any(|word| name.contains(word))
    })
}

// The checksums published beside the release say whether the download arrived whole. A release
// that publishes none, or one that skips this file, is taken without the check.
fn check_it_arrived_whole(assets: &[Asset], downloaded: &str, file: &Path) -> Result<(), String> {
    let Some(list) = find_the_checksums_among(assets) else { return Ok(()) };
    let published = read_a_url(&list.browser_download_url)?;
    let Some(wanted) = find_the_checksum_of(downloaded, &published) else { return Ok(()) };
    let arrived = calculate_the_checksum_of(file)?;
    match arrived == wanted {
        true => Ok(()),
        false => Err(format!(
            "{downloaded} did not arrive whole: the release says its checksum is {wanted} and \
             the file that arrived is {arrived}"
        )),
    }
}

// The shape of a tag is the project's own habit, `v3.7.0` here and `3.7.0` there, so both are
// asked for before giving up on the version.
fn find_the_release_of(repository: &str, version: &str) -> Result<Release, String> {
    let mut refused = Vec::new();
    for tag in [format!("v{version}"), version.to_string()] {
        let url = format!("{GITHUB_API}/{repository}/releases/tags/{tag}");
        match read_a_url(&url) {
            Ok(document) => {
                return serde_json::from_str(&document).map_err(|error| {
                    format!("what github answered about {repository} {tag} does not read: {error}")
                });
            }
            Err(message) => refused.push(message),
        }
    }
    // Both tag shapes fail the same way when the version is not there, and one 404 said twice
    // reads as two faults.
    refused.dedup();
    Err(format!(
        "{repository} has no release tagged v{version} or {version}: {}",
        refused.join("; ")
    ))
}

// A release carries one file per system and architecture, named however the project likes, so the
// right one is chosen by the words in its name and not by any agreed shape.
fn find_the_asset_for_this_machine(assets: &[Asset]) -> Result<&Asset, String> {
    find_the_asset_for(assets, OS, ARCH)
}

fn find_the_asset_for<'a>(assets: &'a [Asset], os: &str, arch: &str) -> Result<&'a Asset, String> {
    let system: &[&str] = match os {
        "windows" => &["windows", "win"],
        "macos" => &["darwin", "macos", "apple", "osx"],
        "linux" => &["linux"],
        other => return Err(format!("{other} is a system this program cannot pick a file for")),
    };
    let machine: &[&str] = match arch {
        "x86_64" => &["x86_64", "amd64", "x64"],
        "aarch64" => &["arm64", "aarch64"],
        other => {
            return Err(format!("{other} is an architecture this program cannot pick a file for"));
        }
    };
    let named = |name: &str, words: &[&str]| {
        words.iter().any(|word| name.contains(&format!("_{word}_")))
    };
    let mut fitting: Vec<&Asset> = assets
        .iter()
        .filter(|asset| {
            let name = cut_into_words(&asset.name);
            named(&name, system) && named(&name, machine)
        })
        .collect();
    fitting.sort_by(|a, b| a.name.cmp(&b.name));
    match fitting.first() {
        Some(asset) => Ok(asset),
        None => Err(format!(
            "this release carries nothing for {os} on {arch}, only {}",
            assets.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
        )),
    }
}

// Every separator made the same, and one at each end, so a word can be looked for whole. Searching
// the name as it stands takes `scc_Darwin_x86_64.tar.gz` for a Windows file, since `darwin` ends
// in `win`.
fn cut_into_words(name: &str) -> String {
    let mut cut = String::from("_");
    for letter in name.chars() {
        cut.push(match letter.is_ascii_alphanumeric() {
            true => letter.to_ascii_lowercase(),
            false => '_',
        });
    }
    cut.push('_');
    cut
}

// The published list is one line per file, the checksum first and the name after it.
fn find_the_checksum_of(asset: &str, published: &str) -> Option<String> {
    published.lines().find_map(|line| {
        let (checksum, named) = line.split_once(char::is_whitespace)?;
        (named.trim() == asset).then(|| checksum.to_lowercase())
    })
}

fn calculate_the_checksum_of(file: &Path) -> Result<String, String> {
    let bytes =
        fs::read(file).map_err(|error| format!("{} could not be read: {error}", file.display()))?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect())
}

// An archive holds the executable at its root or one directory down, so it is looked for.
fn find_the_file_named(named: &str, under: &Path) -> Option<PathBuf> {
    let here = under.join(named);
    if here.is_file() {
        return Some(here);
    }
    fs::read_dir(under)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .find_map(|dir| find_the_file_named(named, &dir))
}

pub(crate) fn read_a_url(url: &str) -> Result<String, String> {
    download_with_curl_or_wget(&["-sSfL", url], &["-qO-", url])
}

fn save_a_url(url: &str, into: &Path) -> Result<(), String> {
    let named = into.display().to_string();
    download_with_curl_or_wget(&["-sSfL", "-o", &named, url], &["-qO", &named, url])?;
    Ok(())
}

// curl ships with Windows and macOS and is on every ordinary Linux; wget is what the smallest
// container images have instead. Only a curl that is not on the machine moves on to wget: falling
// through from a curl that ran and was refused would report a missing wget for a plain 404.
fn download_with_curl_or_wget(curl: &[&str], wget: &[&str]) -> Result<String, String> {
    let mut for_curl = vec!["-A", USER_AGENT];
    for_curl.extend_from_slice(curl);
    let mut for_wget = vec!["-U", USER_AGENT];
    for_wget.extend_from_slice(wget);
    let (curl, wget) = (for_curl.as_slice(), for_wget.as_slice());
    let missing = match run_the_program("curl", curl) {
        Ok(printed) => return Ok(printed),
        Err(Refusal::Refused(message)) => return Err(message),
        Err(Refusal::Missing(message)) => message,
    };
    match run_the_program("wget", wget) {
        Ok(printed) => Ok(printed),
        Err(Refusal::Refused(message)) => Err(message),
        Err(Refusal::Missing(_)) => {
            Err(format!("{missing}, and neither is wget, so there is nothing to download with"))
        }
    }
}

fn run_the_program(program: &str, args: &[&str]) -> Result<String, Refusal> {
    let finished = Command::new(program).args(args).output().map_err(|error| {
        match error.kind() == io::ErrorKind::NotFound {
            true => Refusal::Missing(format!("{program} is not on this machine")),
            false => Refusal::Refused(format!("{program} could not be run: {error}")),
        }
    })?;
    if !finished.status.success() {
        let said = String::from_utf8_lossy(&finished.stderr);
        let said = said.trim();
        return Err(Refusal::Refused(match said.is_empty() {
            true => format!("{program} refused"),
            false => said.to_string(),
        }));
    }
    Ok(String::from_utf8_lossy(&finished.stdout).into_owned())
}

// A program that is not installed and a program that ran and said no are different troubles, and
// only the first is worth trying another program for.
enum Refusal {
    Missing(String),
    Refused(String),
}

impl Refusal {
    fn into_words(self) -> String {
        match self {
            Refusal::Missing(message) | Refusal::Refused(message) => message,
        }
    }
}

fn say(out: &mut dyn Write, message: &str) -> Result<(), String> {
    writeln!(out, "  {}", style::DETAIL.paint(message))
        .map_err(|error| format!("this report could not be written: {error}"))?;
    out.flush().map_err(|error| format!("this report could not be written: {error}"))
}

#[derive(Deserialize)]
struct Release {
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_for_a_machine_is_picked_out_of_what_a_release_carries() {
        let assets = a_release();
        for (os, arch, wanted) in [
            ("windows", "x86_64", "scc_Windows_x86_64.zip"),
            ("windows", "aarch64", "scc_Windows_arm64.zip"),
            ("linux", "x86_64", "scc_Linux_x86_64.tar.gz"),
            ("linux", "aarch64", "scc_Linux_arm64.tar.gz"),
            ("macos", "x86_64", "scc_Darwin_x86_64.tar.gz"),
            ("macos", "aarch64", "scc_Darwin_arm64.tar.gz"),
        ] {
            let picked = find_the_asset_for(&assets, os, arch).unwrap();
            assert_eq!(picked.name, wanted, "{os} on {arch}");
        }
        assert!(find_the_asset_for_this_machine(&assets).is_ok(), "and the machine running this");
    }

    #[test]
    fn a_named_release_file_is_chosen_by_system_and_other_stands_for_the_rest() {
        let how = an_acquisition(&[("windows", "cloc-{version}.exe"), ("other", "cloc-{version}.pl")]);
        assert_eq!(choose_the_file_for(&how, "windows").unwrap(), "cloc-2.10.exe");
        assert_eq!(choose_the_file_for(&how, "linux").unwrap(), "cloc-2.10.pl");
        assert_eq!(choose_the_file_for(&how, "macos").unwrap(), "cloc-2.10.pl");

        let windows_only = an_acquisition(&[("windows", "cloc-{version}.exe")]);
        let refused = choose_the_file_for(&windows_only, "linux").unwrap_err();
        assert!(refused.contains("no release file for linux"), "{refused}");

        let mut unnamed = an_acquisition(&[]);
        unnamed.file = None;
        let refused = choose_the_file_for(&unnamed, "windows").unwrap_err();
        assert!(refused.contains("[acquisition.file]"), "{refused}");
    }

    #[test]
    fn a_release_carrying_nothing_for_this_machine_says_what_it_does_carry() {
        let assets = vec![
            Asset { name: "scc_Plan9_sparc.tar.gz".to_string(), browser_download_url: String::new() },
            Asset { name: "checksums.txt".to_string(), browser_download_url: String::new() },
        ];
        let refused = find_the_asset_for_this_machine(&assets).unwrap_err();
        assert!(refused.contains("scc_Plan9_sparc.tar.gz"), "{refused}");
        assert!(refused.contains(OS), "{refused}");
    }

    #[test]
    fn a_file_name_a_release_hands_over_may_not_carry_a_path_inside_it() {
        let plain = an_asset("scc_Linux_x86_64.tar.gz");
        assert_eq!(name_the_file_of(&plain).unwrap(), "scc_Linux_x86_64.tar.gz");
        for crooked in ["../../somewhere-else.zip", "a/b.zip", "..", ""] {
            assert!(name_the_file_of(&an_asset(crooked)).is_err(), "{crooked}");
        }
    }

    #[test]
    fn the_published_checksums_are_recognised_under_whichever_name_a_project_gives_them() {
        assert_eq!(find_the_checksums_among(&a_release()).unwrap().name, "checksums.txt");

        let shouting = [an_asset("foo_linux_x86_64.tar.gz"), an_asset("SHA256SUMS")];
        assert_eq!(find_the_checksums_among(&shouting).unwrap().name, "SHA256SUMS");

        let none = [an_asset("foo_linux_x86_64.tar.gz")];
        assert!(find_the_checksums_among(&none).is_none(), "and none is not a refusal");
    }

    #[test]
    fn the_checksum_of_one_file_is_read_out_of_the_list_of_all_of_them() {
        let published = "\
9c8f2b1e  scc_Linux_x86_64.tar.gz
AB12CD34  scc_Windows_x86_64.zip
";
        assert_eq!(
            find_the_checksum_of("scc_Windows_x86_64.zip", published),
            Some("ab12cd34".to_string())
        );
        assert_eq!(find_the_checksum_of("scc_Darwin_arm64.tar.gz", published), None);
    }

    #[test]
    fn a_checksum_is_the_sixty_four_characters_of_a_sha_256() {
        let file = std::env::temp_dir().join("linejudge-a_checksum_of_a_known_file");
        fs::write(&file, "abc").unwrap();
        let calculated = calculate_the_checksum_of(&file).unwrap();
        fs::remove_file(&file).unwrap();
        assert_eq!(
            calculated,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn the_executable_is_found_whether_the_archive_held_a_directory_or_not() {
        let root = std::env::temp_dir().join("linejudge-an_unpacked_archive");
        let inner = root.join("scc_3.7.0");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&inner).unwrap();
        fs::write(inner.join("scc"), "a binary").unwrap();
        let found = find_the_file_named("scc", &root);
        let missing = find_the_file_named("tokei", &root);
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(found.unwrap(), inner.join("scc"));
        assert!(missing.is_none());
    }

    #[test]
    fn a_program_this_machine_does_not_have_is_told_apart_from_one_that_ran_and_said_no() {
        match run_the_program("a-program-no-machine-has", &[]) {
            Err(Refusal::Missing(said)) => {
                assert_eq!(said, "a-program-no-machine-has is not on this machine");
            }
            _ => panic!("a program nobody has is missing, not refusing"),
        }
        // Cargo is on any machine running this, since cargo is what runs it.
        match run_the_program("cargo", &["--a-flag-cargo-does-not-have"]) {
            Err(Refusal::Refused(_)) => {}
            _ => panic!("a program that ran and said no is not a missing one"),
        }
    }

    #[test]
    fn a_binary_named_in_the_counters_file_is_said_to_win_over_what_was_downloaded() {
        let mut named = Counters::empty();
        named.name_binary("scc", PathBuf::from("D:/dev/tools/scc.exe"));

        let mut printed = Vec::new();
        report_the_path_that_shadows(&mut printed, "scc", &named).unwrap();
        let said = String::from_utf8(printed).unwrap();
        assert!(said.contains("counters.toml names"), "{said}");
        assert!(said.contains("scc.exe"), "{said}");

        let mut quiet = Vec::new();
        report_the_path_that_shadows(&mut quiet, "tokei", &named).unwrap();
        assert!(quiet.is_empty(), "a counter it does not name has nothing to be said about it");
    }

    fn a_release() -> Vec<Asset> {
        [
            "checksums.txt",
            "scc_Darwin_arm64.tar.gz",
            "scc_Darwin_x86_64.tar.gz",
            "scc_Linux_arm64.tar.gz",
            "scc_Linux_i386.tar.gz",
            "scc_Linux_x86_64.tar.gz",
            "scc_Windows_arm64.zip",
            "scc_Windows_i386.zip",
            "scc_Windows_x86_64.zip",
        ]
        .into_iter()
        .map(an_asset)
        .collect()
    }

    fn an_asset(name: &str) -> Asset {
        Asset {
            name: name.to_string(),
            browser_download_url: format!("https://example.invalid/{name}"),
        }
    }

    fn an_acquisition(files: &[(&str, &str)]) -> Acquisition {
        Acquisition {
            channel: GITHUB_RELEASE_FILE.to_string(),
            name: "AlDanial/cloc".to_string(),
            version: "2.10".to_string(),
            file: Some(
                files.iter().map(|(system, name)| (system.to_string(), name.to_string())).collect(),
            ),
        }
    }
}
