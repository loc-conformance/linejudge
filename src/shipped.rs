//! The corpus this build carries, for a project that depends on the crate and has no checkout.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use crate::corpus::CASES_DIR;

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

/// Whether this directory holds the cases this build carries. Compared by contents and not by
/// path, because a checkout points every command at its own working copy and that copy is what the
/// build was made from. Nothing may be missing and nothing may be extra: `--corpus` swaps the
/// whole corpus, so a directory holding half our cases is a different corpus.
pub fn holds_the_carried_cases(dir: &Path) -> bool {
    let Some(found) = collect_every_file_under(dir) else { return false };
    found.len() == count_the_files_carried_under(CASES_DIR)
        && found.iter().all(|relative| is_what_was_carried(dir, relative, CASES_DIR))
}

/// Whether a directory given for the adapters, the dialects or the records changes any of what
/// this build carries under `name_of_dir`. A file that is missing is no change, unlike in a
/// corpus: these three replace only the counters they name.
pub fn replaces_nothing_carried_under(dir: &Path, name_of_dir: &str) -> bool {
    let Some(found) = collect_every_file_under(dir) else { return false };
    found.iter().all(|relative| is_what_was_carried(dir, relative, name_of_dir))
}

fn collect_every_file_under(dir: &Path) -> Option<Vec<PathBuf>> {
    let mut found = Vec::new();
    walk_every_file_under(dir, dir, &mut found)?;
    Some(found)
}

// file_type does not follow a symlink, so a link pointing back up cannot loop the walk for ever.
fn walk_every_file_under(dir: &Path, root: &Path, found: &mut Vec<PathBuf>) -> Option<()> {
    for entry in fs::read_dir(dir).ok()?.filter_map(|entry| entry.ok()) {
        match entry.file_type().ok()?.is_dir() {
            true => walk_every_file_under(&entry.path(), root, found)?,
            false => found.push(entry.path().strip_prefix(root).ok()?.to_path_buf()),
        }
    }
    Some(())
}

fn is_what_was_carried(dir: &Path, relative: &Path, name_of_dir: &str) -> bool {
    let named = format!("{name_of_dir}/{}", relative.display().to_string().replace('\\', "/"));
    let Some((_, contents)) = FILES.iter().find(|(carried, _)| *carried == named) else {
        return false;
    };
    let Ok(found) = fs::read_to_string(dir.join(relative)) else { return false };
    found.replace("\r\n", "\n") == contents.replace("\r\n", "\n")
}

fn count_the_files_carried_under(name_of_dir: &str) -> usize {
    FILES
        .iter()
        .filter(|(relative, _)| {
            relative.strip_prefix(name_of_dir).is_some_and(|rest| rest.starts_with('/'))
        })
        .count()
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
        fs::remove_dir_all(&root).unwrap();
        assert!(wrong.is_empty(), "{wrong:?}");
    }

    #[test]
    fn a_corpus_is_ours_by_what_is_in_it_and_not_by_where_it_sits() {
        let root = env::temp_dir().join("linejudge-a_corpus_by_what_is_inside_it");
        let _ = fs::remove_dir_all(&root);
        write_the_shipped_files_into(&root).unwrap();
        let cases = root.join(CASES_DIR);
        let elsewhere = holds_the_carried_cases(&cases);

        let extra = cases.join("one_more.toml");
        fs::write(&extra, "a case nobody here carries").unwrap();
        let with_more = holds_the_carried_cases(&cases);
        fs::remove_file(&extra).unwrap();

        fs::remove_file(cases.join("readings.toml")).unwrap();
        let with_less = holds_the_carried_cases(&cases);
        fs::remove_dir_all(&root).unwrap();

        assert!(elsewhere, "a copy of our cases is our cases, wherever it was put");
        assert!(!with_more, "a corpus with ours and more of its own is not ours");
        assert!(!with_less, "a corpus missing one of ours is not ours");
    }

    #[test]
    fn a_folder_naming_one_counter_replaces_nothing_and_an_edited_file_does() {
        let root = env::temp_dir().join("linejudge-a_layer_that_replaces_nothing");
        let _ = fs::remove_dir_all(&root);
        write_the_shipped_files_into(&root).unwrap();

        let one = root.join("one-counter");
        let named = FILES
            .iter()
            .find(|(relative, _)| relative.starts_with(&format!("{DIALECTS_DIR}/")))
            .map(|(relative, _)| relative.trim_start_matches(&format!("{DIALECTS_DIR}/")))
            .unwrap();
        let copied = one.join(named);
        fs::create_dir_all(copied.parent().unwrap()).unwrap();
        fs::copy(root.join(DIALECTS_DIR).join(named), &copied).unwrap();
        let a_part_of_it = replaces_nothing_carried_under(&one, DIALECTS_DIR);

        fs::write(&copied, "rules of my own\n").unwrap();
        let edited = replaces_nothing_carried_under(&one, DIALECTS_DIR);

        fs::copy(root.join(DIALECTS_DIR).join(named), &copied).unwrap();
        fs::write(one.join("mycounter.toml"), "a counter nobody here carries").unwrap();
        let added = replaces_nothing_carried_under(&one, DIALECTS_DIR);
        fs::remove_dir_all(&root).unwrap();

        assert!(a_part_of_it, "one counter's file, unchanged, leaves every other counter alone");
        assert!(!edited, "the same file with something else in it replaces what we carry");
        assert!(!added, "a file under a name we carry nothing of adds a counter of its own");
    }

    // The names live twice: once in build.rs, which cannot import them because it runs before the
    // library is compiled, and once as the constants a consumer joins onto the directory. This is
    // what holds the two lists together.
    #[test]
    fn what_was_carried_sits_under_exactly_the_named_directories() {
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
