//! What a counter says about one file.

use std::collections::BTreeMap;

/// Everything one counter says about one file. The rules derive one of these, a recorded file
/// holds one, and running a counter gives a third, all the same shape so any two can be compared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    /// The file taken as a whole, embedded languages included.
    pub counts: Counts,
    /// One entry per language found inside the file, empty for a file that holds only its own.
    pub regions: Vec<RegionCounts>,
}

/// A set of counts, with the buckets named as the dialect that produced them names them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Counts {
    /// Physical lines, which is every line of the file whatever is on it.
    pub lines: u32,
    /// How many lines went to each bucket, `code` and `comments` and `blanks` for most counters.
    pub buckets: BTreeMap<String, u32>,
}

/// The counts of one stretch of another language, a script inside a page say.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegionCounts {
    /// The language as the counter names it, so `JavaScript` rather than `js`.
    pub language: String,
    /// Lines of this language only, and never of the file around it.
    pub lines: u32,
    /// The same buckets as the file's own counts, holding only this language's lines.
    pub buckets: BTreeMap<String, u32>,
}
