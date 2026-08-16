use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

const COMMENT: char = '#';
const DIALECT_SEPARATOR: char = ':';

/// The cases a counter already fails, named one per line in its own repository, so that its build
/// breaks on a failure nobody has seen before and on nothing else.
pub struct KnownFailures {
    named: BTreeSet<(Option<String>, String)>,
}

impl KnownFailures {
    pub fn read(path: &Path) -> Result<KnownFailures, String> {
        let text = fs::read_to_string(path)
            .map_err(|e| format!("{} could not be read: {e}", path.display()))?;
        KnownFailures::of(&text)
    }

    pub fn of(text: &str) -> Result<KnownFailures, String> {
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
        Ok(KnownFailures { named })
    }

    pub fn names(&self, dialect: &str, case: &str) -> bool {
        self.named.contains(&(None, case.to_string()))
            || self.named.contains(&(Some(dialect.to_string()), case.to_string()))
    }

    pub fn entries(&self) -> impl Iterator<Item = (Option<&str>, &str)> {
        self.named.iter().map(|(d, c)| (d.as_deref(), c.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A_LIST: &str = "\
# the ones we know about
2400
region:2300      # only one of the two models fails this

4900
";

    #[test]
    fn a_case_named_alone_is_named_for_every_dialect() {
        let known = KnownFailures::of(A_LIST).unwrap();
        assert!(known.names("content", "2400"));
        assert!(known.names("region", "2400"));
        assert!(known.names("default", "4900"));
    }

    #[test]
    fn a_case_named_with_a_dialect_is_named_for_that_one_only() {
        let known = KnownFailures::of(A_LIST).unwrap();
        assert!(known.names("region", "2300"));
        assert!(!known.names("content", "2300"));
    }

    #[test]
    fn comments_and_empty_lines_name_nothing() {
        let known = KnownFailures::of(A_LIST).unwrap();
        assert!(!known.names("content", "the"));
        assert_eq!(known.entries().count(), 3);
    }
}
