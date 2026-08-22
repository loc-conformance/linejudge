//! The corpus this build carries, for a project that depends on the crate and has no checkout.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

include!(concat!(env!("OUT_DIR"), "/shipped.rs"));

const APP_DIR: &str = "linejudge";
const PARTIAL_SUFFIX: &str = ".partial";

/// Writes out the corpus, the adapters, the dialects and the recorded answers this build carries.
/// The directory is named after a hash of its contents, so a build whose corpus changed does not
/// read the copy an older one left behind.
pub fn create_the_shipped_dir() -> Result<PathBuf, String> {
    let mut refused = Vec::new();
    for root in find_the_app_dirs() {
        let dir = root.join(HASH);
        if dir.is_dir() {
            return Ok(dir);
        }
        match write_the_shipped_files_beside(&dir) {
            Ok(()) => return Ok(dir),
            Err(message) => refused.push(message),
        }
    }
    Err(format!("what this build carries could not be written out: {}", refused.join("; ")))
}

/// Everywhere linejudge keeps files of its own, best first, ending at the temporary directory so a
/// build runner with no home still has somewhere. Whatever writes and whatever reads has to walk
/// this list in this order.
pub fn find_the_app_dirs() -> Vec<PathBuf> {
    [find_the_data_dir(), Some(env::temp_dir())]
        .into_iter()
        .flatten()
        .map(|root| root.join(APP_DIR))
        .collect()
}

fn write_the_shipped_files_into(dir: &Path) -> Result<(), String> {
    for (relative, contents) in FILES {
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("{} could not be made: {e}", parent.display()))?;
        }
        fs::write(&path, contents)
            .map_err(|e| format!("{} could not be written: {e}", path.display()))?;
    }
    Ok(())
}

// Written to one side and moved into place, so a run stopped halfway leaves no directory the next
// one would find and trust.
fn write_the_shipped_files_beside(dir: &Path) -> Result<(), String> {
    let partial = dir.with_file_name(format!("{HASH}{PARTIAL_SUFFIX}"));
    let _ = fs::remove_dir_all(&partial);
    write_the_shipped_files_into(&partial)?;
    fs::rename(&partial, dir).map_err(|e| format!("{} could not be made: {e}", dir.display()))
}

fn find_the_data_dir() -> Option<PathBuf> {
    let named = |name: &str| env::var_os(name).map(PathBuf::from).filter(|path| path.is_absolute());
    if cfg!(windows) {
        return named("APPDATA");
    }
    if cfg!(target_os = "macos") {
        return named("HOME").map(|home| home.join("Library").join("Application Support"));
    }
    named("XDG_DATA_HOME").or_else(|| named("HOME").map(|home| home.join(".local").join("share")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::ADAPTERS_DIR;
    use crate::corpus::CASES_DIR;
    use crate::dialects::DIALECTS_DIR;
    use crate::recorded::RECORDED_DIR;

    #[test]
    fn what_is_written_out_is_what_the_repository_holds() {
        let root = env::temp_dir().join("linejudge-what_this_build_carries");
        let _ = fs::remove_dir_all(&root);
        write_the_shipped_files_into(&root).unwrap();
        let checkout = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut wrong = Vec::new();
        for (relative, contents) in FILES {
            if !root.join(relative).exists() {
                wrong.push(format!("{relative} was not written"));
            }
            let its_own = fs::read_to_string(checkout.join(relative)).unwrap();
            if its_own.replace("\r\n", "\n") != contents.replace("\r\n", "\n") {
                wrong.push(format!("{relative} is not what the checkout holds"));
            }
        }
        let cases = FILES.iter().filter(|(name, _)| name.ends_with("case.toml")).count();
        fs::remove_dir_all(&root).unwrap();
        assert!(wrong.is_empty(), "{wrong:?}");
        assert_eq!(cases, 83, "the corpus that was carried holds {cases} cases");
    }

    // The four names live twice: once in build.rs, which cannot import them because it runs before
    // the library is compiled, and once as the constants a consumer joins onto the directory. This
    // is what holds the two lists together.
    #[test]
    fn what_was_carried_sits_under_exactly_the_four_named_directories() {
        let mut top: Vec<&str> =
            FILES.iter().filter_map(|(relative, _)| relative.split('/').next()).collect();
        top.sort_unstable();
        top.dedup();
        let mut named = [ADAPTERS_DIR, CASES_DIR, DIALECTS_DIR, RECORDED_DIR];
        named.sort_unstable();
        assert_eq!(top, named);
    }

    #[test]
    fn a_write_stopped_halfway_leaves_nothing_the_next_run_would_trust() {
        let root = env::temp_dir().join("linejudge-a_write_stopped_halfway");
        let dir = root.join(HASH);
        let partial = root.join(format!("{HASH}{PARTIAL_SUFFIX}"));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(partial.join("cases")).unwrap();
        fs::write(partial.join("cases").join("readings.toml"), "half a file").unwrap();

        write_the_shipped_files_beside(&dir).unwrap();
        let readings = fs::read_to_string(dir.join("cases").join("readings.toml")).unwrap();
        let left = partial.exists();
        fs::remove_dir_all(&root).unwrap();
        assert!(readings.contains("[rust-doc-comment]"), "{readings}");
        assert!(!left, "the half-written directory is still there");
    }
}
