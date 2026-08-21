use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

const SHIPPED_DIRS: [&str; 4] = ["adapters", "cases", "dialects", "recorded"];

const FNV_START: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn main() {
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("cargo names the package root"));
    let mut files = Vec::new();
    for name in SHIPPED_DIRS {
        collect_every_file_under(&root.join(name), &root, &mut files);
    }
    files.sort();

    let mut hash = FNV_START;
    let mut table = String::new();
    for (relative, absolute) in &files {
        let contents = fs::read(absolute).unwrap_or_else(|e| panic!("{}: {e}", absolute.display()));
        for byte in relative.as_bytes().iter().chain(&contents) {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(FNV_PRIME);
        }
        let path = absolute.display().to_string().replace('\\', "/");
        writeln!(table, "    ({relative:?}, include_str!({path:?})),").expect("a string grows");
    }

    let generated = format!(
        "const HASH: &str = \"{hash:016x}\";\n\nstatic FILES: &[(&str, &str)] = &[\n{table}];\n"
    );
    let out = PathBuf::from(env::var("OUT_DIR").expect("cargo names the output directory"));
    fs::write(out.join("shipped.rs"), generated).expect("the generated table is written");
}

/// Every directory is named as well as every file, since adding a case changes the modification
/// time of the group directory holding it and of nothing else, and a build that did not notice
/// would carry the corpus as it stood at the last one.
fn collect_every_file_under(dir: &Path, root: &Path, found: &mut Vec<(String, PathBuf)>) {
    println!("cargo:rerun-if-changed={}", dir.display());
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
    for path in entries.filter_map(|entry| entry.ok()).map(|entry| entry.path()) {
        if path.is_dir() {
            collect_every_file_under(&path, root, found);
            continue;
        }
        println!("cargo:rerun-if-changed={}", path.display());
        let relative = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string()
            .replace('\\', "/");
        found.push((relative, path));
    }
}
