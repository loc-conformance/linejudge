//! The questions two counters are allowed to answer differently.

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use serde::Deserialize;

pub(crate) const READINGS_FILE: &str = "readings.toml";

/// Every reading a truth may mark as optional, as the corpus's own file defines them. A reading is
/// a question two counters may fairly answer differently, whether a Rust doc comment is its own
/// language of Markdown being one.
#[derive(Debug)]
pub struct Readings {
    readings: BTreeMap<String, Reading>,
}

/// One such question.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reading {
    /// What the question is, written so that a refusal can quote it at whoever hit it.
    pub sentence: String,
    /// The case that shows the question being asked, named as the corpus names it.
    pub witness: String,
}

impl Readings {
    /// Reads `readings.toml` from a corpus directory. No such file is no readings, which is not an
    /// error: it is what any corpus that has never needed one looks like.
    pub fn read(dir: &Path) -> Result<Readings, String> {
        let path = dir.join(READINGS_FILE);
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                return Ok(Readings { readings: BTreeMap::new() });
            }
            Err(error) => return Err(format!("{} could not be read: {error}", path.display())),
        };
        let readings: BTreeMap<String, Reading> = toml::from_str(&text)
            .map_err(|error| format!("{} does not parse: {error}", path.display()))?;
        for (name, reading) in &readings {
            if reading.sentence.trim().is_empty() {
                return Err(format!(
                    "{}: {name} says nothing about what it is",
                    path.display()
                ));
            }
            if reading.witness.trim().is_empty() {
                return Err(format!("{}: {name} names no witness case", path.display()));
            }
        }
        Ok(Readings { readings })
    }

    /// The reading of that name, and `None` where this corpus defines none.
    pub fn find(&self, name: &str) -> Option<&Reading> {
        self.readings.get(name)
    }

    /// Every reading by name, in alphabetical order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Reading)> {
        self.readings.iter()
    }
}

#[cfg(test)]
pub(crate) fn read_the_shipped_readings() -> Readings {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("cases");
    Readings::read(&dir).unwrap_or_else(|message| panic!("{message}"))
}

#[cfg(test)]
mod tests {
    use std::env;
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn the_shipped_readings_are_read_and_each_carries_its_sentence_and_witness() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("cases");
        let readings = Readings::read(&dir).unwrap();
        let named: Vec<&String> = readings.iter().map(|(name, _)| name).collect();
        assert_eq!(named, ["rust-doc-comment", "vue-template"]);
        let doc = readings.find("rust-doc-comment").unwrap();
        assert!(doc.sentence.contains("doc comment"), "{}", doc.sentence);
        assert_eq!(doc.witness, "8040-doc_comment_with_no_text");
    }

    #[test]
    fn a_corpus_without_the_file_defines_no_reading_and_an_empty_sentence_is_refused() {
        let dir = env::temp_dir().join("linejudge-readings_missing_and_empty");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let none = Readings::read(&dir).unwrap();
        assert_eq!(none.iter().count(), 0);

        fs::write(
            dir.join(READINGS_FILE),
            "[js-jsdoc]\nsentence = \" \"\nwitness = \"0100-a_case\"\n",
        )
        .unwrap();
        let refused = Readings::read(&dir)
            .err()
            .unwrap_or_else(|| panic!("an empty sentence was read anyway"));
        fs::remove_dir_all(&dir).unwrap();
        assert!(refused.contains("js-jsdoc says nothing about what it is"), "{refused}");
    }
}
