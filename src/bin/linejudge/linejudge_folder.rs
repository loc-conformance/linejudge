use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use linejudge::adapter::{ADAPTERS_DIR, EXPLAIN_SCRIPTS_DIR};
use linejudge::corpus::CASES_DIR;
use linejudge::dialects::DIALECTS_DIR;
use linejudge::recorded::RECORDED_DIR;

pub const FOLDER_NAME: &str = ".linejudge";
pub const COUNTERS_FILE: &str = "counters.toml";
pub const KNOWN_FAILURES_DIR: &str = "known-failures";

const SETTINGS_FILE: &str = "settings.toml";

// The `.linejudge` folder, found by walking up from the working directory the way cargo finds its
// own. It holds what was not said on the command line: `counters.toml` naming the binaries, and
// the declaration itself under fixed names, `cases`, `adapters`, `dialects`, `explain-scripts`,
// `recorded` and a `known-failures` directory of one `<counter>.txt` per counter, each taken
// whenever it exists. `settings.toml` names one of those that lives elsewhere and beats the fixed
// name. A flag beats everything in the folder, and the folder beats the defaults.
pub struct Folder {
    dir: PathBuf,
    settings: Settings,
}

impl Folder {
    pub fn find(start: &Path) -> Result<Option<Folder>, String> {
        let mut dir = start;
        loop {
            let candidate = dir.join(FOLDER_NAME);
            if candidate.is_dir() {
                let settings = Settings::read(&candidate.join(SETTINGS_FILE))?;
                return Ok(Some(Folder { dir: candidate, settings }));
            }
            match dir.parent() {
                Some(parent) => dir = parent,
                None => return Ok(None),
            }
        }
    }

    pub fn find_corpus(&self) -> Option<PathBuf> {
        self.resolve(self.settings.corpus.as_deref()).or_else(|| self.find_inside(CASES_DIR))
    }

    pub fn find_adapters(&self) -> Option<PathBuf> {
        self.resolve(self.settings.adapters.as_deref()).or_else(|| self.find_inside(ADAPTERS_DIR))
    }

    pub fn find_dialects(&self) -> Option<PathBuf> {
        self.resolve(self.settings.dialects.as_deref()).or_else(|| self.find_inside(DIALECTS_DIR))
    }

    pub fn find_explain_scripts(&self) -> Option<PathBuf> {
        self.resolve(self.settings.explain_scripts.as_deref())
            .or_else(|| self.find_inside(EXPLAIN_SCRIPTS_DIR))
    }

    pub fn find_recorded(&self) -> Option<PathBuf> {
        self.resolve(self.settings.recorded.as_deref()).or_else(|| self.find_inside(RECORDED_DIR))
    }

    // The single-file flag path only. A file names one counter's failures, so which counter is
    // read from `--counter`; the per-counter directory below carries the counter in its own name.
    pub fn find_known_failures(&self) -> Option<PathBuf> {
        self.resolve(self.settings.known_failures.as_deref())
    }

    pub fn find_known_failures_dir(&self) -> Option<PathBuf> {
        self.find_inside(KNOWN_FAILURES_DIR)
    }

    pub fn get_counters_file(&self) -> PathBuf {
        self.dir.join(COUNTERS_FILE)
    }

    // Every relative path in the folder resolves against this, so a committed path works from any
    // subdirectory.
    pub fn get_root(&self) -> &Path {
        self.dir.parent().unwrap_or(&self.dir)
    }

    fn resolve(&self, path: Option<&str>) -> Option<PathBuf> {
        let path = Path::new(path?);
        match path.is_absolute() {
            true => Some(path.to_path_buf()),
            false => Some(self.get_root().join(path)),
        }
    }

    // Only a directory is taken by its fixed name, so a stray file that happens to share one is
    // passed over rather than handed on to be read as a directory and refused.
    fn find_inside(&self, named: &str) -> Option<PathBuf> {
        let inside = self.dir.join(named);
        inside.is_dir().then_some(inside)
    }
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    corpus: Option<String>,
    adapters: Option<String>,
    dialects: Option<String>,
    #[serde(rename = "explain-scripts")]
    explain_scripts: Option<String>,
    recorded: Option<String>,
    #[serde(rename = "known-failures")]
    known_failures: Option<String>,
}

impl Settings {
    fn read(path: &Path) -> Result<Settings, String> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Settings::default()),
            Err(error) => return Err(format!("{} could not be read: {error}", path.display())),
        };
        toml::from_str(&text).map_err(|e| format!("{} does not parse: {e}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    #[test]
    fn the_folder_is_found_by_climbing_out_of_a_subdirectory() {
        let root = env::temp_dir().join("linejudge-a_folder_to_find");
        let deep = root.join("a").join("b");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(FOLDER_NAME)).unwrap();
        fs::create_dir_all(&deep).unwrap();
        let folder = Folder::find(&deep).unwrap().unwrap_or_else(|| panic!("nothing found"));
        assert_eq!(folder.get_root(), root);
        assert_eq!(folder.get_counters_file(), root.join(FOLDER_NAME).join(COUNTERS_FILE));
        assert!(folder.find_corpus().is_none());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_relative_path_in_the_settings_resolves_against_the_folders_root() {
        let root = env::temp_dir().join("linejudge-a_folder_with_settings");
        let absolute = env::temp_dir().join("known.txt").display().to_string().replace('\\', "/");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(FOLDER_NAME)).unwrap();
        fs::write(
            root.join(FOLDER_NAME).join(SETTINGS_FILE),
            format!("corpus = \"cases\"\nknown-failures = \"{absolute}\"\n"),
        )
        .unwrap();
        let folder = Folder::find(&root).unwrap().unwrap_or_else(|| panic!("nothing found"));
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(folder.find_corpus().unwrap(), root.join("cases"));
        assert_eq!(folder.find_known_failures().unwrap(), PathBuf::from(&absolute));
        assert!(folder.find_dialects().is_none());
    }

    #[test]
    fn what_sits_inside_the_folder_under_a_fixed_name_needs_no_settings_line() {
        let root = env::temp_dir().join("linejudge-a_folder_with_fixed_names");
        let inside = root.join(FOLDER_NAME);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(inside.join(ADAPTERS_DIR)).unwrap();
        fs::create_dir_all(inside.join(DIALECTS_DIR)).unwrap();
        fs::create_dir_all(inside.join(KNOWN_FAILURES_DIR)).unwrap();
        // A stray file sharing a fixed name is passed over, not taken and later refused.
        fs::write(inside.join(RECORDED_DIR), "").unwrap();
        let folder = Folder::find(&root).unwrap().unwrap_or_else(|| panic!("nothing found"));
        let found = (
            folder.find_adapters(),
            folder.find_dialects(),
            folder.find_known_failures_dir(),
            folder.find_explain_scripts(),
            folder.find_corpus(),
            folder.find_recorded(),
            folder.find_known_failures(),
        );
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(found.0.unwrap(), inside.join(ADAPTERS_DIR));
        assert_eq!(found.1.unwrap(), inside.join(DIALECTS_DIR));
        assert_eq!(found.2.unwrap(), inside.join(KNOWN_FAILURES_DIR));
        assert!(found.3.is_none());
        assert!(found.4.is_none());
        assert!(found.5.is_none(), "a plain file sharing a fixed name is not taken");
        assert!(found.6.is_none(), "the per-counter directory is not the single-file flag");
    }

    #[test]
    fn a_settings_line_beats_the_fixed_name_inside_the_folder() {
        let root = env::temp_dir().join("linejudge-a_folder_naming_its_own_path");
        let inside = root.join(FOLDER_NAME);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(inside.join(ADAPTERS_DIR)).unwrap();
        fs::write(inside.join(SETTINGS_FILE), "adapters = \"elsewhere\"\n").unwrap();
        let folder = Folder::find(&root).unwrap().unwrap_or_else(|| panic!("nothing found"));
        fs::remove_dir_all(&root).unwrap();
        assert_eq!(folder.find_adapters().unwrap(), root.join("elsewhere"));
    }

    #[test]
    fn a_setting_this_folder_does_not_know_is_refused() {
        let root = env::temp_dir().join("linejudge-a_folder_with_a_typo");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(FOLDER_NAME)).unwrap();
        fs::write(root.join(FOLDER_NAME).join(SETTINGS_FILE), "corpsu = \"cases\"\n").unwrap();
        let refused = match Folder::find(&root) {
            Ok(_) => panic!("the typo was read anyway"),
            Err(refused) => refused,
        };
        fs::remove_dir_all(&root).unwrap();
        assert!(refused.contains("does not parse"), "{refused}");
    }

}
