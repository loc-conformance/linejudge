//! The cases a counter is allowed to fail.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const COMMENT: char = '#';
const DIALECT_SEPARATOR: char = ':';

/// The cases a counter already fails, one per line in its own repository, so its build breaks on a
/// failure nobody has seen before and on nothing else. A case is named the way the report names
/// it, so a line of the report is a line of this file.
#[derive(Debug)]
pub struct KnownFailures {
    named: BTreeSet<(Option<String>, String)>,
}

impl KnownFailures {
    /// Reads the list from a file. A file that is not there is an error: an empty list is written
    /// as an empty file, and a path that does not exist is a path somebody got wrong.
    pub fn read(path: &Path) -> Result<KnownFailures, String> {
        let text = fs::read_to_string(path)
            .map_err(|e| format!("{} could not be read: {e}", path.display()))?;
        Ok(KnownFailures::of(&text))
    }

    /// Reads the list from the text itself. One case per line, `#` starting a comment, and
    /// `<dialect>:<case>` naming a case for one way of counting instead of all of them. An entry
    /// that turns out to name no case of the corpus is reported by the run, not refused here.
    pub fn of(text: &str) -> KnownFailures {
        let mut named = BTreeSet::new();
        for line in text.lines() {
            let entry = match line.split_once(COMMENT) {
                Some((before, _)) => before.trim(),
                None => line.trim(),
            };
            if entry.is_empty() {
                continue;
            }
            match entry.split_once(DIALECT_SEPARATOR) {
                Some((dialect, case)) => {
                    named.insert((Some(dialect.trim().to_string()), case.trim().to_string()))
                }
                None => named.insert((None, entry.to_string())),
            };
        }
        KnownFailures { named }
    }

    /// Whether this case is allowed to fail in this way of counting.
    pub fn names(&self, name_of_dialect: &str, name_of_case: &str) -> bool {
        self.named.contains(&(None, name_of_case.to_string()))
            || self
                .named
                .contains(&(Some(name_of_dialect.to_string()), name_of_case.to_string()))
    }

    /// Every line of the list, as the dialect it named and the case it named, for a report that
    /// wants to say an entry matches no case of the corpus.
    pub fn entries(&self) -> impl Iterator<Item = (Option<&str>, &str)> {
        self.named.iter().map(|(d, c)| (d.as_deref(), c.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A_LIST: &str = "\
# the ones we know about
8010-punctuation_only_line
region:8020-blank_line_inside_block_comment      # only one of the two models fails this

8040-doc_comment_with_no_text
";

    #[test]
    fn a_case_named_alone_is_named_for_every_dialect() {
        let known = KnownFailures::of(A_LIST);
        assert!(known.names("content", "8010-punctuation_only_line"));
        assert!(known.names("region", "8010-punctuation_only_line"));
        assert!(known.names("default", "8040-doc_comment_with_no_text"));
    }

    #[test]
    fn a_case_named_with_a_dialect_is_named_for_that_one_only() {
        let known = KnownFailures::of(A_LIST);
        assert!(known.names("region", "8020-blank_line_inside_block_comment"));
        assert!(!known.names("content", "8020-blank_line_inside_block_comment"));
    }

    #[test]
    fn the_number_alone_names_no_case() {
        let known = KnownFailures::of(A_LIST);
        assert!(!known.names("content", "8010"));
    }

    #[test]
    fn comments_and_empty_lines_name_nothing() {
        let known = KnownFailures::of(A_LIST);
        assert!(!known.names("content", "the"));
        assert_eq!(known.entries().count(), 3);
    }
}
