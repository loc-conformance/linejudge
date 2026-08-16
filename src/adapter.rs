use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::buckets::find_buckets;
use crate::measurement::{Measurement, OutputFormat, read_output};

const ADAPTER_EXTENSION: &str = "toml";
const FILE_PLACEHOLDER: &str = "{file}";
const VERSION_FLAG: &str = "--version";
const UNKNOWN_VERSION: &str = "unknown version";

// An Adapter symbolizes the bridge between a loc counter tool, and this program.
// Every field of it is declared in adapters/<name_of_counter>.toml: the shape that tool prints its
// answer in, the arguments it is run with, the flag that asks it for its version, where its binary
// comes from, and one block per way it counts.
#[derive(Debug)]
pub struct Adapter {
    // a 'counter' is a loc counting tool, like mezura or tokei
    pub name_of_counter: String,
    pub output: OutputFormat,
    pub args: Vec<String>,
    pub version_flag: Option<String>,
    pub acquisition: Acquisition,
    pub dialects: Vec<Dialect>,
}

impl Adapter {
    pub fn read_one(dir: &Path, name_of_counter: &str) -> Result<Adapter, String> {
        let path = dir.join(format!("{name_of_counter}.{ADAPTER_EXTENSION}"));
        if !path.is_file() {
            return Err(format!("{} does not exist, so {name_of_counter} is a counter this suite cannot run", path.display()));
        }
        Adapter::read(&path)
    }

    pub fn read_all(dir: &Path) -> Result<Vec<Adapter>, String> {
        let entries =
            fs::read_dir(dir).map_err(|e| format!("{} could not be opened: {e}", dir.display()))?;
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == ADAPTER_EXTENSION))
            .collect();
        paths.sort();
        let mut adapters = Vec::new();
        for path in paths {
            adapters.push(Adapter::read(&path)?);
        }
        Ok(adapters)
    }

    fn read(path: &Path) -> Result<Adapter, String> {
        let text = fs::read_to_string(path)
            .map_err(|e| format!("{} could not be read: {e}", path.display()))?;
        let raw: RawAdapter = toml::from_str(&text)
            .map_err(|e| format!("{} does not parse: {e}", path.display()))?;
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        if stem != raw.name {
            return Err(format!(
                "{} names the counter {}, and a counter's adapter is the file named after it",
                path.display(),
                raw.name
            ));
        }
        if !raw.args.iter().any(|a| a == FILE_PLACEHOLDER) {
            return Err(format!("{} names no {FILE_PLACEHOLDER} to count", path.display()));
        }
        let mut dialects = Vec::new();
        for (name, dialect) in raw.dialect {
            let Some(buckets) = find_buckets(&raw.name, &name) else {
                return Err(format!("{}: {} is a dialect this suite has no buckets for", path.display(), name));
            };
            if let Some(misplaced) = dialect.args.iter().find(|a| a.contains(FILE_PLACEHOLDER)) {
                return Err(format!(
                    "{}: {} names {misplaced} among its own arguments, and the file belongs in the \
                     arguments every dialect shares",
                    path.display(),
                    name
                ));
            }
            dialects.push(Dialect { name, args: dialect.args, third_bucket: buckets[2] });
        }
        if dialects.is_empty() {
            return Err(format!("{} names no way of counting to run", path.display()));
        }
        dialects.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Adapter {
            name_of_counter: raw.name,
            output: raw.output,
            args: raw.args,
            version_flag: match raw.version_flag {
                Some(flag) if flag.is_empty() => None,
                Some(flag) => Some(flag),
                None => Some(VERSION_FLAG.to_string()),
            },
            acquisition: raw.acquisition,
            dialects,
        })
    }

    pub fn measure(
        &self,
        dialect: &Dialect,
        binary: &Path,
        file: &Path,
    ) -> Result<Option<Measurement>, String> {
        let args = self.build_args(dialect, file);
        let printed = run_counter(binary, &args)?;
        read_output(self.output, dialect.third_bucket, &printed)
            .map_err(|e| format!("{} on {}: {e}", self.name_of_counter, file.display()))
    }

    /// The version is a label on the report and never a condition of the run, so a binary that
    /// answers the flag with an error, or declares no flag at all, comes out "unknown version".
    pub fn read_version_or_unknown(&self, binary: &Path) -> String {
        let Some(flag) = &self.version_flag else { return UNKNOWN_VERSION.to_string() };
        match run_counter(binary, std::slice::from_ref(flag)) {
            Ok(printed) if !printed.trim().is_empty() => printed.trim().to_string(),
            _ => UNKNOWN_VERSION.to_string(),
        }
    }

    pub fn format_command(&self, dialect: &Dialect, binary: &Path, file: &Path) -> String {
        format!("{} {}", binary.display(), self.build_args(dialect, file).join(" "))
    }

    fn build_args(&self, dialect: &Dialect, file: &Path) -> Vec<String> {
        let mut args: Vec<String> = self
            .args
            .iter()
            .map(|a| a.replace(FILE_PLACEHOLDER, &file.display().to_string()))
            .collect();
        args.extend(dialect.args.iter().cloned());
        args
    }
}

#[derive(Debug)]
pub struct Dialect {
    pub name: String,
    pub args: Vec<String>,
    pub third_bucket: &'static str,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Acquisition {
    pub channel: String,
    pub name: String,
}

// Only stdout is read. mezura writes its warnings to stderr, and a counter that says something
// there while answering correctly has said nothing about the file.
fn run_counter(binary: &Path, args: &[String]) -> Result<String, String> {
    let output = Command::new(binary)
        .args(args)
        .output()
        .map_err(|e| format!("{} could not be run: {e}", binary.display()))?;
    if !output.status.success() {
        return Err(format!("{} exited with {}", binary.display(), output.status));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("{} printed no text: {e}", binary.display()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAdapter {
    name: String,
    output: OutputFormat,
    args: Vec<String>,
    #[serde(rename = "version-flag")]
    version_flag: Option<String>,
    acquisition: Acquisition,
    dialect: std::collections::BTreeMap<String, RawDialect>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDialect {
    args: Vec<String>,
}

#[cfg(test)]
mod tests {
    use std::env;

    use super::*;

    #[test]
    fn every_shipped_adapter_is_read_and_names_a_dialect_this_suite_knows() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters");
        let adapters = Adapter::read_all(&dir).unwrap();
        let names: Vec<&str> = adapters.iter().map(|a| a.name_of_counter.as_str()).collect();
        assert_eq!(names, ["mezura", "scc", "tokei"]);
        let mezura = &adapters[0];
        let dialects: Vec<&str> = mezura.dialects.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(dialects, ["content", "region"]);
        assert_eq!(mezura.dialects[0].third_bucket, "extra");
        assert_eq!(mezura.dialects[1].third_bucket, "blanks");
        assert_eq!(adapters[2].acquisition.channel, "crates-io");
    }

    #[test]
    fn one_counter_named_opens_its_own_file_and_no_other() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters");
        assert_eq!(Adapter::read_one(&dir, "scc").unwrap().name_of_counter, "scc");
        let missing = Adapter::read_one(&dir, "cloc").unwrap_err();
        assert!(missing.contains("cannot run"), "{missing}");
    }

    #[test]
    fn an_adapter_under_a_name_that_is_not_its_own_is_refused() {
        let path = write_an_adapter(
            "an_adapter_under_the_wrong_name",
            "name = \"scc\"\noutput = \"scc-json\"\nargs = [\"{file}\"]\n\
             [acquisition]\nchannel = \"crates-io\"\nname = \"scc\"\n[dialect.default]\nargs = []\n",
        );
        let refused = Adapter::read(&path).unwrap_err();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        assert!(refused.contains("the file named after it"), "{refused}");
    }

    #[test]
    fn the_file_takes_the_place_of_its_placeholder_and_the_dialect_speaks_last() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters");
        let mezura = &Adapter::read_all(&dir).unwrap()[0];
        let args = mezura.build_args(&mezura.dialects[1], Path::new("a/case/input.rs"));
        assert_eq!(args[0], "a/case/input.rs");
        assert_eq!(args[args.len() - 2..], ["--counting".to_string(), "region".to_string()]);
    }

    #[test]
    fn a_version_nobody_answers_is_unknown_and_stops_nothing() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters");
        let tokei = &Adapter::read_all(&dir).unwrap()[2];
        assert_eq!(tokei.version_flag.as_deref(), Some("--version"));
        assert_eq!(tokei.read_version_or_unknown(Path::new("a-binary-that-does-not-exist")), "unknown version");

        let path = write_an_adapter(
            "an_adapter_with_no_version_flag",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\nversion-flag = \"\"\n\
             [acquisition]\nchannel = \"crates-io\"\nname = \"tokei\"\n[dialect.default]\nargs = []\n",
        );
        let unversioned = Adapter::read(&path).unwrap();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        assert_eq!(unversioned.version_flag, None);
        assert_eq!(unversioned.read_version_or_unknown(Path::new("irrelevant")), "unknown version");
    }

    #[test]
    fn an_adapter_that_names_no_way_of_counting_is_refused() {
        let path = write_an_adapter(
            "an_adapter_with_no_dialect",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\n\
             [acquisition]\nchannel = \"crates-io\"\nname = \"tokei\"\n[dialect]\n",
        );
        let refused = Adapter::read(&path).unwrap_err();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        assert!(refused.contains("no way of counting"), "{refused}");
    }

    #[test]
    fn the_file_named_among_one_dialects_own_arguments_is_refused() {
        let path = write_an_adapter(
            "an_adapter_naming_the_file_twice",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\n\
             [acquisition]\nchannel = \"crates-io\"\nname = \"tokei\"\n\
             [dialect.default]\nargs = [\"{file}\"]\n",
        );
        let refused = Adapter::read(&path).unwrap_err();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        assert!(refused.contains("the arguments every dialect shares"), "{refused}");
    }

    #[test]
    fn an_adapter_that_never_names_the_file_is_refused() {
        let path = write_an_adapter(
            "an_adapter_with_no_file",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"--output\", \"json\"]\n\
             [acquisition]\nchannel = \"crates-io\"\nname = \"tokei\"\n[dialect.default]\nargs = []\n",
        );
        let refused = Adapter::read(&path).unwrap_err();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        assert!(refused.contains("{file}"), "{refused}");
    }

    #[test]
    fn an_adapter_naming_a_dialect_with_no_buckets_is_refused() {
        let path = write_an_adapter(
            "an_adapter_with_an_unknown_dialect",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\n\
             [acquisition]\nchannel = \"crates-io\"\nname = \"tokei\"\n[dialect.strict]\nargs = []\n",
        );
        let refused = Adapter::read(&path).unwrap_err();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        assert!(refused.contains("no buckets for"), "{refused}");
    }

    fn write_an_adapter(name: &str, text: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("linejudge-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tokei.toml");
        fs::write(&path, text).unwrap();
        path
    }
}
