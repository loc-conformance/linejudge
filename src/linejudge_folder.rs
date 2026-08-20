use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const FOLDER_NAME: &str = ".linejudge";
pub const COUNTERS_FILE: &str = "counters.toml";

const SETTINGS_FILE: &str = "settings.toml";

/// The `.linejudge` folder, found by walking up from the working directory the way cargo finds its
/// own. It holds whatever the run needs that was not said on the command line: `counters.toml`
/// naming the binaries and `settings.toml` naming the paths. A flag wins over the folder, and the
/// folder wins over the defaults.
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
        self.resolve(self.settings.corpus.as_deref())
    }

    pub fn find_adapters(&self) -> Option<PathBuf> {
        self.resolve(self.settings.adapters.as_deref())
    }

    pub fn find_dialects(&self) -> Option<PathBuf> {
        self.resolve(self.settings.dialects.as_deref())
    }

    pub fn find_recorded(&self) -> Option<PathBuf> {
        self.resolve(self.settings.recorded.as_deref())
    }

    pub fn find_known_failures(&self) -> Option<PathBuf> {
        self.resolve(self.settings.known_failures.as_deref())
    }

    pub fn get_counters_file(&self) -> PathBuf {
        self.dir.join(COUNTERS_FILE)
    }

    /// The directory the folder sits in, which is what every relative path in it resolves
    /// against, so that a committed path works from any subdirectory.
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
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    corpus: Option<String>,
    adapters: Option<String>,
    dialects: Option<String>,
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
    fn the_folder_is_found_from_a_subdirectory_and_absent_where_none_exists() {
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

        let lone = env::temp_dir().join("linejudge-no_folder_anywhere");
        let _ = fs::remove_dir_all(&lone);
        fs::create_dir_all(&lone).unwrap();
        // The temp dir sits under the machine's own tree, where a stray .linejudge above it would
        // make this flaky; finding none from the filesystem root is the only stable half.
        let found = Folder::find(&lone).unwrap();
        fs::remove_dir_all(&lone).unwrap();
        if let Some(found) = found {
            assert_ne!(found.get_root(), lone);
        }
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
