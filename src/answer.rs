use std::collections::BTreeMap;

/// Everything one counter says about one file: the counts of the file and the counts of each
/// stretch of another language inside it. The rules work one out from a case's marked spans, a
/// case file records the one a counter printed, and running a counter produces a third, and all
/// three are this shape so that any two of them can be compared.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Answer {
    pub counts: Counts,
    pub regions: Vec<RegionCounts>,
}

/// How many lines a file has and how many of them went to each bucket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Counts {
    pub lines: u32,
    pub buckets: BTreeMap<String, u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RegionCounts {
    pub language: String,
    pub lines: u32,
    pub buckets: BTreeMap<String, u32>,
}
