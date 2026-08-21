use std::env::consts::EXE_SUFFIX;
use std::fs;
use std::path::{Path, PathBuf};

use linejudge::shipped::find_the_app_dirs;

const BIN_DIR: &str = "bin";
const PARTIAL_SUFFIX: &str = ".partial";

// A downloaded counter lives where linejudge keeps its own files and never in anybody's tree. The
// version is part of the directory name, so two projects that pinned two versions do not overwrite
// one another.
pub fn find_the_binary_of(name_of_counter: &str, version: &str) -> Option<PathBuf> {
    let named = format!("{name_of_counter}{EXE_SUFFIX}");
    find_the_app_dirs()
        .into_iter()
        .map(|root| name_the_dir_under(&root, name_of_counter, version).join(&named))
        .find(|path| path.is_file())
}

// A download is assembled beside where it will end up and never in the place itself, so a fetch
// stopped halfway leaves no directory the next run would find and trust.
pub fn create_a_partial_dir_for(name_of_counter: &str, version: &str) -> Result<PathBuf, String> {
    let mut refused = Vec::new();
    for root in find_the_app_dirs() {
        let named = format!("{name_of_counter}-{version}{PARTIAL_SUFFIX}");
        let dir = root.join(BIN_DIR).join(named);
        let _ = fs::remove_dir_all(&dir);
        match fs::create_dir_all(&dir) {
            Ok(()) => return Ok(dir),
            Err(error) => refused.push(format!("{}: {error}", dir.display())),
        }
    }
    Err(format!("there is nowhere to put what is downloaded: {}", refused.join("; ")))
}

// Renaming it to the name the lookup goes by is the moment a download counts as done. Anything
// already under that name is thrown away first: a directory without the binary in it would be
// found and answer nothing.
pub fn finish_the_partial_dir(partial: &Path) -> Result<PathBuf, String> {
    let name = partial
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_suffix(PARTIAL_SUFFIX))
        .ok_or_else(|| format!("{} is not a half-finished download", partial.display()))?;
    let dir = partial.with_file_name(name);
    let _ = fs::remove_dir_all(&dir);
    fs::rename(partial, &dir)
        .map_err(|error| format!("{} could not be put in place: {error}", dir.display()))?;
    Ok(dir)
}

fn name_the_dir_under(root: &Path, name_of_counter: &str, version: &str) -> PathBuf {
    root.join(BIN_DIR).join(format!("{name_of_counter}-{version}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_downloaded_counter_is_found_under_its_own_name_and_the_version_it_was_pinned_at() {
        let counter = "a-counter-no-machine-has";
        let root = find_the_app_dirs().last().unwrap().join(BIN_DIR);
        let dir = root.join(format!("{counter}-1.2.3"));
        let binary = dir.join(format!("{counter}{EXE_SUFFIX}"));
        let _ = fs::remove_dir_all(&dir);
        assert!(find_the_binary_of(counter, "1.2.3").is_none());

        fs::create_dir_all(&dir).unwrap();
        fs::write(&binary, "not really a binary").unwrap();
        let found = find_the_binary_of(counter, "1.2.3");
        let other = find_the_binary_of(counter, "1.2.4");
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(found.unwrap(), binary);
        assert!(other.is_none(), "another version is another directory");
    }

    #[test]
    fn a_download_stopped_halfway_leaves_nothing_the_next_run_would_trust() {
        let counter = "another-counter-no-machine-has";
        let partial = create_a_partial_dir_for(counter, "2.0.0").unwrap();
        fs::write(partial.join("half-a-file"), "not really a binary").unwrap();
        assert!(find_the_binary_of(counter, "2.0.0").is_none(), "half of one is not one");

        let started_again = create_a_partial_dir_for(counter, "2.0.0").unwrap();
        let left = started_again.join("half-a-file").exists();
        fs::write(started_again.join(format!("{counter}{EXE_SUFFIX}")), "a binary").unwrap();
        let dir = finish_the_partial_dir(&started_again).unwrap();
        let found = find_the_binary_of(counter, "2.0.0");
        fs::remove_dir_all(&dir).unwrap();
        assert!(!left, "the second attempt started from what the first left");
        assert_eq!(found.unwrap(), dir.join(format!("{counter}{EXE_SUFFIX}")));
    }
}
