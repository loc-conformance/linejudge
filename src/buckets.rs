use std::collections::BTreeMap;

/// Every counter this suite knows, every way it counts, and what that way calls its three buckets.
/// A case file, an adapter file and a counter's own output are all held to this one roster.
const DIALECT_BUCKETS: [(&str, &str, [&str; 3]); 4] = [
    ("mezura", "content", ["code", "comments", "extra"]),
    ("mezura", "region", ["code", "comments", "blanks"]),
    ("scc", "default", ["code", "comments", "blanks"]),
    ("tokei", "default", ["code", "comments", "blanks"]),
];

pub fn find_buckets(counter: &str, dialect: &str) -> Option<[&'static str; 3]> {
    DIALECT_BUCKETS
        .iter()
        .find(|(c, d, _)| *c == counter && *d == dialect)
        .map(|(_, _, buckets)| *buckets)
}

pub fn find_every_dialect() -> impl Iterator<Item = (&'static str, &'static str)> {
    DIALECT_BUCKETS.iter().map(|(counter, dialect, _)| (*counter, *dialect))
}

pub fn check_buckets(found: &BTreeMap<String, u32>, wanted: &[&str; 3]) -> Result<(), String> {
    for name in wanted {
        if !found.contains_key(*name) {
            return Err(format!("has no {name} bucket, and this dialect has {}", wanted.join(", ")));
        }
    }
    for name in found.keys() {
        if !wanted.contains(&name.as_str()) {
            return Err(format!(
                "has a bucket named {name}, and this dialect has {}",
                wanted.join(", ")
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dialect_off_the_roster_has_no_buckets() {
        assert_eq!(find_buckets("mezura", "content").unwrap()[2], "extra");
        assert_eq!(find_buckets("tokei", "default").unwrap()[2], "blanks");
        assert!(find_buckets("tokei", "strict").is_none());
        assert!(find_buckets("cloc", "default").is_none());
        assert_eq!(find_every_dialect().count(), 4);
    }

    #[test]
    fn a_bucket_set_is_exactly_the_dialects_own_and_says_which_when_it_is_not() {
        let wanted = ["code", "comments", "blanks"];
        let named = |names: [(&str, u32); 3]| {
            names.iter().map(|(n, v)| (n.to_string(), *v)).collect::<BTreeMap<_, _>>()
        };
        assert!(check_buckets(&named([("code", 1), ("comments", 1), ("blanks", 0)]), &wanted).is_ok());
        let missing = check_buckets(&named([("code", 1), ("comments", 1), ("extra", 0)]), &wanted);
        assert!(missing.unwrap_err().contains("no blanks bucket"));
    }
}
