use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

const BOM: &str = "\u{feff}";

/// Where each counter's binary sits, named once so that neither a person nor a workflow has to
/// write the path again.
#[derive(Debug)]
pub struct Counters {
    binaries: BTreeMap<String, PathBuf>,
}

impl Counters {
    pub fn empty() -> Counters {
        Counters { binaries: BTreeMap::new() }
    }

    /// A file that is not there is not an error, and every other way of failing to read one is,
    /// since a file that is there and unreadable is a file somebody wrote on purpose.
    pub fn read(path: &Path) -> Result<Counters, String> {
        let text = match fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Counters::empty()),
            Err(error) => return Err(format!("{} could not be read: {error}", path.display())),
        };
        let named: BTreeMap<String, String> = toml::from_str(text.trim_start_matches(BOM))
            .map_err(|e| format!("{} does not parse: {e}", path.display()))?;
        Ok(Counters {
            binaries: named.into_iter().map(|(k, v)| (k, PathBuf::from(v))).collect(),
        })
    }

    pub fn find_binary(&self, counter: &str) -> Option<&Path> {
        self.binaries.get(counter).map(|p| p.as_path())
    }

    pub fn name_binary(&mut self, counter: &str, binary: PathBuf) {
        self.binaries.insert(counter.to_string(), binary);
    }

    /// A relative path in a committed counters file, `target/release/...` say, means it from the
    /// file's own project however deep the working directory sits.
    pub fn resolve_against(&mut self, root: &Path) {
        for binary in self.binaries.values_mut() {
            if binary.is_relative() {
                *binary = root.join(&binary);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    #[test]
    fn a_machine_with_no_counters_file_has_no_counters_and_no_error() {
        let missing = env::temp_dir().join("linejudge-no-counters-file-here.toml");
        let _ = fs::remove_file(&missing);
        assert!(Counters::read(&missing).unwrap().find_binary("tokei").is_none());
    }

    #[test]
    fn a_named_binary_is_found_and_a_flag_replaces_it() {
        let path = env::temp_dir().join("linejudge-counters.toml");
        fs::write(&path, "tokei = \"D:/dev/tools/tokei.exe\"\n").unwrap();
        let mut counters = Counters::read(&path).unwrap();
        assert_eq!(counters.find_binary("tokei"), Some(Path::new("D:/dev/tools/tokei.exe")));
        counters.name_binary("tokei", PathBuf::from("elsewhere/tokei.exe"));
        assert_eq!(counters.find_binary("tokei"), Some(Path::new("elsewhere/tokei.exe")));
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn a_relative_binary_resolves_against_the_root_and_an_absolute_one_stays() {
        let mut counters = Counters::empty();
        counters.name_binary("mezura", PathBuf::from("target/release/mezura.exe"));
        counters.name_binary("tokei", PathBuf::from("D:/dev/tools/tokei.exe"));
        counters.resolve_against(Path::new("D:/somewhere"));
        assert_eq!(
            counters.find_binary("mezura"),
            Some(Path::new("D:/somewhere/target/release/mezura.exe"))
        );
        assert_eq!(counters.find_binary("tokei"), Some(Path::new("D:/dev/tools/tokei.exe")));
    }

    #[test]
    fn a_file_that_is_there_and_cannot_be_read_says_so_instead_of_reading_as_empty() {
        let dir = env::temp_dir().join("linejudge-counters-that-is-a-directory.toml");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let refused = Counters::read(&dir).unwrap_err();
        fs::remove_dir_all(&dir).unwrap();
        assert!(refused.contains("could not be read"), "{refused}");
    }

    #[test]
    fn a_byte_order_mark_in_front_of_the_first_name_is_not_part_of_it() {
        let path = env::temp_dir().join("linejudge-counters-with-a-bom.toml");
        fs::write(&path, "\u{feff}tokei = \"tokei\"\n").unwrap();
        let counters = Counters::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        assert_eq!(counters.find_binary("tokei"), Some(Path::new("tokei")));
    }
}
