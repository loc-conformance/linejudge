use std::env::consts::EXE_SUFFIX;
use std::path::PathBuf;

use crate::shipped::find_the_app_dirs;

const BIN_DIR: &str = "bin";

/// A downloaded counter lives where this program keeps its own files rather than in anybody's
/// tree, and the version is part of the directory name because two projects that pinned two
/// versions must not overwrite one another.
pub fn find_the_binary_of(counter: &str, version: &str) -> Option<PathBuf> {
    let under = format!("{counter}-{version}");
    let named = format!("{counter}{EXE_SUFFIX}");
    find_the_app_dirs()
        .into_iter()
        .map(|root| root.join(BIN_DIR).join(&under).join(&named))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use std::fs;

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
}
