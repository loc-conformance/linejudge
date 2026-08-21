//! How to run one counter and read what it prints.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::answer::Answer;
use crate::dialects::Dialects;
use crate::locator::{Locator, RawLocator};
use crate::measurement::{OutputFormat, read_output};
use crate::per_line::PerLineFormat;

/// The directory the adapters are read from, one `<counter>.toml` inside it per counter.
pub const ADAPTERS_DIR: &str = "adapters";

pub(crate) const UNKNOWN_VERSION: &str = "unknown version";

const ADAPTER_EXTENSION: &str = "toml";
const FILE_PLACEHOLDER: &str = "{file}";
const VERSION_FLAG: &str = "--version";

/// Everything needed to run one counter and read its answer, as `adapters/<counter>.toml`
/// declares it.
#[derive(Debug)]
pub struct Adapter {
    pub name_of_counter: String,
    /// Where the counter itself lives, for a report that links to it. `None` is not linked.
    pub repository: Option<String>,
    /// The command line it is run with, `{file}` standing for the file, and the arguments of the
    /// chosen way of counting appended after these.
    pub args: Vec<String>,
    /// The command line that asks the counter for its own reading of a file line by line, used in
    /// place of `args`. `None` is a counter with no such command, which is not a failure.
    pub explain_args: Option<Vec<String>>,
    /// The format that reading is printed in, where it is one this suite can compare line by line.
    /// `None` is a counter whose reading is text for a person and nothing more.
    pub explain_output: Option<PerLineFormat>,
    /// Show only the lines of it holding this text, each from the text to its end. Where nothing
    /// holds it, everything is shown and the report says so.
    pub explain_keep_from: Option<String>,
    /// The flag that asks the counter for its version, `None` for one that answers no such flag.
    pub version_flag: Option<String>,
    /// `None` is a counter that cannot be fetched: it is left out of any scheduled sweep, loudly,
    /// and still runs for anyone holding its binary.
    pub acquisition: Option<Acquisition>,
    /// One per way this counter counts, each named after the dialect holding its rules.
    pub invocations: Vec<Invocation>,
}

impl Adapter {
    /// The directories are layered: the last one holding `<counter>.toml` is the one that
    /// describes it, so somebody whose tool moved a flag writes one file and not a copy of ours.
    pub fn read_one(
        dirs: &[PathBuf],
        name_of_counter: &str,
        dialects: &Dialects,
    ) -> Result<Adapter, String> {
        let named = format!("{name_of_counter}.{ADAPTER_EXTENSION}");
        match dirs.iter().rev().map(|dir| dir.join(&named)).find(|path| path.is_file()) {
            Some(path) => Adapter::read(&path, dialects),
            None => Err(format!(
                "no {named} in {}, so {name_of_counter} is a counter this suite cannot run",
                name_every_one_of(dirs)
            )),
        }
    }

    /// Reads every adapter the directories hold, layered the same way, in alphabetical order.
    pub fn read_all(dirs: &[PathBuf], dialects: &Dialects) -> Result<Vec<Adapter>, String> {
        let mut found: BTreeMap<String, PathBuf> = BTreeMap::new();
        for dir in dirs {
            let entries = fs::read_dir(dir)
                .map_err(|e| format!("{} could not be opened: {e}", dir.display()))?;
            for path in entries
                .filter_map(|entry| entry.ok())
                .map(|entry| entry.path())
                .filter(|path| path.extension().is_some_and(|e| e == ADAPTER_EXTENSION))
            {
                let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
                found.insert(stem.unwrap_or_default(), path);
            }
        }
        found.values().map(|path| Adapter::read(path, dialects)).collect()
    }

    fn read(path: &Path, dialects: &Dialects) -> Result<Adapter, String> {
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
        if let Some(explain) = &raw.explain_args
            && !explain.iter().any(|a| a == FILE_PLACEHOLDER)
        {
            return Err(format!(
                "{} names no {FILE_PLACEHOLDER} in its explain-args",
                path.display()
            ));
        }
        match &raw.explain_keep_from {
            Some(_) if raw.explain_args.is_none() => {
                return Err(format!(
                    "{} declares explain-keep-from with no explain-args to trim",
                    path.display()
                ));
            }
            Some(keep) if keep.is_empty() => {
                return Err(format!(
                    "{} declares an empty explain-keep-from, which keeps everything, so leave \
                     the field out",
                    path.display()
                ));
            }
            Some(_) if raw.explain_output.is_some() => {
                return Err(format!(
                    "{} declares both explain-output and explain-keep-from, and they are two ways \
                     of taking the same output: one reads it as a document, the other picks lines \
                     out of it for a person to read",
                    path.display()
                ));
            }
            _ => {}
        }
        if raw.explain_output.is_some() && raw.explain_args.is_none() {
            return Err(format!(
                "{} declares explain-output with no explain-args to print it",
                path.display()
            ));
        }
        let mut ways = Vec::new();
        let mut output_is_read = false;
        for (name, dialect) in raw.dialect {
            let Some(found) = dialects.find(&raw.name, &name) else {
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
            let reader = match dialect.read {
                Some(read) => Reader::Declared(Box::new(
                    Locator::of(read, &found.buckets).map_err(|e| {
                        format!("{}: the read block of {name} {e}", path.display())
                    })?,
                )),
                None => match raw.output {
                    Some(format) => {
                        output_is_read = true;
                        Reader::Written(format)
                    }
                    None => {
                        return Err(format!(
                            "{}: {name} has no read block and the adapter names no output, so \
                             nothing says how what the counter prints is read",
                            path.display()
                        ));
                    }
                },
            };
            ways.push(Invocation {
                name,
                args: dialect.args,
                buckets: found.buckets.clone(),
                reader,
            });
        }
        if ways.is_empty() {
            return Err(format!("{} names no way of counting to run", path.display()));
        }
        if raw.output.is_some() && !output_is_read {
            return Err(format!(
                "{} names an output, and every dialect declares its own read block, so leave the \
                 field out",
                path.display()
            ));
        }
        ways.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Adapter {
            name_of_counter: raw.name,
            repository: raw.repository,
            args: raw.args,
            explain_args: raw.explain_args,
            explain_output: raw.explain_output,
            explain_keep_from: raw.explain_keep_from,
            version_flag: match raw.version_flag {
                Some(flag) if flag.is_empty() => None,
                Some(flag) => Some(flag),
                None => Some(VERSION_FLAG.to_string()),
            },
            acquisition: raw.acquisition,
            invocations: ways,
        })
    }

    /// Runs the counter over one file and reads its answer. `None` is a counter that says there is
    /// no such file, which is an answer of its own and not a failure.
    pub fn measure(
        &self,
        invocation: &Invocation,
        binary: &Path,
        file: &Path,
    ) -> Result<Option<Answer>, String> {
        let args = self.build_args(invocation, file);
        let printed = run_counter(binary, &args)?;
        match &invocation.reader {
            Reader::Written(format) => read_output(*format, &invocation.buckets, &printed),
            Reader::Declared(locator) => locator.read(&printed),
        }
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

    /// Runs the counter's own per-line command and hands back what it printed, as it printed it.
    /// `None` is an adapter that declares no such command.
    pub fn run_explain(
        &self,
        invocation: &Invocation,
        binary: &Path,
        file: &Path,
    ) -> Option<Result<String, String>> {
        let base = self.explain_args.as_ref()?;
        Some(run_counter(binary, &build_command_args(base, invocation, file)))
    }

    /// The command as a person would retype it, meant to be pasted into a shell. Paths under the
    /// directory the run works from are written relative to it and the rest are left whole.
    pub fn format_command(&self, invocation: &Invocation, binary: &Path, file: &Path) -> String {
        let args = self.build_args(invocation, &shorten_the_path(file));
        format!("{} {}", shorten(binary), args.join(" "))
    }

    /// The same for the per-line command, and `None` where the adapter declares none.
    pub fn format_explain_command(
        &self,
        invocation: &Invocation,
        binary: &Path,
        file: &Path,
    ) -> Option<String> {
        let base = self.explain_args.as_ref()?;
        let args = build_command_args(base, invocation, &shorten_the_path(file));
        Some(format!("{} {}", shorten(binary), args.join(" ")))
    }

    fn build_args(&self, invocation: &Invocation, file: &Path) -> Vec<String> {
        build_command_args(&self.args, invocation, file)
    }
}

/// How this counter is asked for one of its ways of counting, and how what it prints is read. The
/// rules that way of counting is judged by are the [`crate::dialects::Dialect`] of the same name.
#[derive(Debug)]
pub struct Invocation {
    /// `default` for a counter that counts only the one way.
    pub name: String,
    /// What is added to the counter's command line to ask for this way of counting.
    pub args: Vec<String>,
    /// In the order its dialect file lists them.
    pub buckets: Vec<String>,
    pub(crate) reader: Reader,
}

#[derive(Debug)]
pub(crate) enum Reader {
    Written(OutputFormat),
    Declared(Box<Locator>),
}

/// Where a build of the counter is downloaded from.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Acquisition {
    /// `github-release-asset` for a project that attaches built files to a release, `crates-io`
    /// for one that publishes only source and has to be compiled.
    pub channel: String,
    /// What the channel calls it: the `owner/repo` on GitHub, or the name of the crate.
    pub name: String,
    /// Downloaded exactly, never resolved to whatever is newest, so two runs on different days
    /// measure the same build.
    pub version: String,
}

/// Whether a binary's version line is the version that was declared. It cannot be an equality: a
/// counter prints its own name and build details around the number, `tokei 14.0.0 compiled with
/// serialization support: json` beside `scc version 3.7.0`. Neither end of the match may sit
/// against a digit or a dot, or a declared `4.0.0` would answer yes to `tokei 14.0.0`.
pub fn is_the_declared_version(declared: &str, printed: &str) -> bool {
    let stands_alone = |beside: Option<char>| !beside.is_some_and(|c| c.is_ascii_digit() || c == '.');
    printed.match_indices(declared).any(|(at, _)| {
        stands_alone(printed[..at].chars().next_back())
            && stands_alone(printed[at + declared.len()..].chars().next())
    })
}

fn shorten(path: &Path) -> String {
    shorten_the_path(path).display().to_string()
}

fn shorten_the_path(path: &Path) -> PathBuf {
    let Ok(here) = env::current_dir() else { return path.to_path_buf() };
    path.strip_prefix(&here).unwrap_or(path).to_path_buf()
}

fn name_every_one_of(dirs: &[PathBuf]) -> String {
    dirs.iter().map(|dir| dir.display().to_string()).collect::<Vec<_>>().join(" or ")
}

fn build_command_args(base: &[String], invocation: &Invocation, file: &Path) -> Vec<String> {
    let mut args: Vec<String> = base
        .iter()
        .map(|a| a.replace(FILE_PLACEHOLDER, &file.display().to_string()))
        .collect();
    args.extend(invocation.args.iter().cloned());
    args
}

// Only stdout is read. mezura writes its warnings to stderr, and a counter that says something
// there while answering correctly has said nothing about the file.
fn run_counter(binary: &Path, args: &[String]) -> Result<String, String> {
    let output = Command::new(binary)
        .args(args)
        .output()
        .map_err(|e| format!("{} could not be run: {e}", binary.display()))?;
    // What it printed while failing is all that is known about why, and a counter is free to have
    // wrapped one sentence over as many lines as it liked.
    if !output.status.success() {
        let printed = String::from_utf8_lossy(&output.stderr);
        let said: Vec<&str> =
            printed.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
        let exited = format!("{} exited with {}", binary.display(), output.status);
        return Err(match said.is_empty() {
            true => exited,
            false => format!("{exited}: {}", said.join(" ")),
        });
    }
    String::from_utf8(output.stdout).map_err(|e| format!("{} printed no text: {e}", binary.display()))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAdapter {
    name: String,
    repository: Option<String>,
    output: Option<OutputFormat>,
    args: Vec<String>,
    #[serde(rename = "explain-args")]
    explain_args: Option<Vec<String>>,
    #[serde(rename = "explain-output")]
    explain_output: Option<PerLineFormat>,
    #[serde(rename = "explain-keep-from")]
    explain_keep_from: Option<String>,
    #[serde(rename = "version-flag")]
    version_flag: Option<String>,
    acquisition: Option<Acquisition>,
    dialect: std::collections::BTreeMap<String, RawDialect>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawDialect {
    args: Vec<String>,
    read: Option<RawLocator>,
}

#[cfg(test)]
mod tests {
    use std::env;

    use crate::dialects::read_the_shipped_dialects;

    use super::*;

    #[test]
    fn every_shipped_adapter_is_read_and_names_a_dialect_this_suite_knows() {
        let dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters")];
        let adapters = Adapter::read_all(&dirs, &read_the_shipped_dialects()).unwrap();
        let names: Vec<&str> = adapters.iter().map(|a| a.name_of_counter.as_str()).collect();
        assert_eq!(names, ["mezura", "scc", "tokei"]);
        let mezura = &adapters[0];
        let dialects: Vec<&str> = mezura.invocations.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(dialects, ["content", "region"]);
        assert_eq!(mezura.invocations[0].buckets, ["code", "comments", "extra"]);
        assert_eq!(mezura.invocations[1].buckets, ["code", "comments", "blanks"]);
        let tokei = adapters[2].acquisition.as_ref().unwrap();
        assert_eq!((tokei.channel.as_str(), tokei.version.as_str()), ("crates-io", "14.0.0"));
        for adapter in &adapters {
            let home = adapter.repository.as_deref().unwrap_or_default();
            assert!(home.starts_with("https://"), "{}: {home}", adapter.name_of_counter);
        }
        for dialect in mezura.invocations.iter().chain(&adapters[1].invocations) {
            assert!(matches!(dialect.reader, Reader::Declared(_)), "{}", dialect.name);
        }
        assert!(matches!(adapters[2].invocations[0].reader, Reader::Written(OutputFormat::TokeiJson)));
    }

    #[test]
    fn the_command_a_report_prints_is_written_from_where_the_run_works() {
        let dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters")];
        let adapters = Adapter::read_all(&dirs, &read_the_shipped_dialects()).unwrap();
        let tokei = adapters.iter().find(|a| a.name_of_counter == "tokei").unwrap();
        let here = env::current_dir().unwrap();

        let under = here.join("cases").join("0400-a_case").join("input.c");
        let printed = tokei.format_command(&tokei.invocations[0], Path::new("tokei"), &under);
        assert!(printed.contains(&format!("cases{}0400-a_case", std::path::MAIN_SEPARATOR)),
                "{printed}");
        assert!(!printed.contains(&here.display().to_string()), "{printed}");

        let elsewhere = Path::new("/somewhere/of/its/own/input.c");
        let whole = tokei.format_command(&tokei.invocations[0], Path::new("tokei"), elsewhere);
        assert!(whole.contains("somewhere"), "{whole}");
    }

    // A counter nobody publishes anywhere has no channel to declare, and all the absence costs is
    // that nothing can fetch it.
    #[test]
    fn an_adapter_with_no_acquisition_reads_as_a_counter_that_cannot_be_fetched() {
        let path = write_an_adapter(
            "an_adapter_that_cannot_be_fetched",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\n\
             [dialect.default]\nargs = []\n",
        );
        let adapter = Adapter::read(&path, &read_the_shipped_dialects()).unwrap();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        assert!(adapter.acquisition.is_none());
    }

    #[test]
    fn a_dialect_nothing_says_how_to_read_is_refused_and_so_is_an_output_nobody_reads() {
        let unread = write_an_adapter(
            "an_adapter_saying_nothing_about_reading",
            "name = \"tokei\"\nargs = [\"{file}\"]\n\
             [dialect.default]\nargs = []\n",
        );
        let refused = Adapter::read(&unread, &read_the_shipped_dialects()).unwrap_err();
        fs::remove_dir_all(unread.parent().unwrap()).unwrap();
        assert!(refused.contains("nothing says how"), "{refused}");

        let unused = write_an_adapter(
            "an_adapter_whose_output_nobody_reads",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\n\
             [dialect.default]\nargs = []\n\
             [dialect.default.read]\neach = \"[]\"\nlines = \"Lines\"\ncode = \"Code\"\n\
             comments = \"Comment\"\nblanks = \"Blank\"\n",
        );
        let refused = Adapter::read(&unused, &read_the_shipped_dialects()).unwrap_err();
        fs::remove_dir_all(unused.parent().unwrap()).unwrap();
        assert!(refused.contains("leave the field out"), "{refused}");
    }

    #[test]
    fn a_broken_read_block_is_refused_with_the_file_and_the_dialect_named() {
        let path = write_an_adapter(
            "an_adapter_with_a_broken_read_block",
            "name = \"tokei\"\nargs = [\"{file}\"]\n\
             [dialect.default]\nargs = []\n\
             [dialect.default.read]\neach = \"[]\"\nlines = \"Lines\"\ncode = \"Code\"\n\
             comments = \"Comment\"\n",
        );
        let refused = Adapter::read(&path, &read_the_shipped_dialects()).unwrap_err();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        assert!(refused.contains("the read block of default"), "{refused}");
        assert!(refused.contains("no path for blanks"), "{refused}");
    }

    #[test]
    fn one_counter_named_opens_its_own_file_and_no_other() {
        let dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters")];
        let dialects = read_the_shipped_dialects();
        assert_eq!(Adapter::read_one(&dirs, "scc", &dialects).unwrap().name_of_counter, "scc");
        let missing = Adapter::read_one(&dirs, "cloc", &dialects).unwrap_err();
        assert!(missing.contains("cannot run"), "{missing}");
    }

    #[test]
    fn an_adapter_under_a_name_that_is_not_its_own_is_refused() {
        let path = write_an_adapter(
            "an_adapter_under_the_wrong_name",
            "name = \"scc\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\n\
             [dialect.default]\nargs = []\n",
        );
        let refused = Adapter::read(&path, &read_the_shipped_dialects()).unwrap_err();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        assert!(refused.contains("the file named after it"), "{refused}");
    }

    #[test]
    fn the_file_takes_the_place_of_its_placeholder_and_the_dialect_speaks_last() {
        let dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters")];
        let mezura = &Adapter::read_all(&dirs, &read_the_shipped_dialects()).unwrap()[0];
        let args = mezura.build_args(&mezura.invocations[1], Path::new("a/case/input.rs"));
        assert_eq!(args[0], "a/case/input.rs");
        assert_eq!(args[args.len() - 2..], ["--counting".to_string(), "region".to_string()]);
    }

    #[test]
    fn the_per_line_command_is_declared_per_counter_and_has_to_name_the_file() {
        let dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters")];
        let adapters = Adapter::read_all(&dirs, &read_the_shipped_dialects()).unwrap();
        let scc = &adapters[1];
        let command = scc
            .format_explain_command(&scc.invocations[0], Path::new("scc.exe"), Path::new("input.py"))
            .unwrap();
        assert_eq!(command, "scc.exe -t --no-cocomo -f csv input.py");
        assert_eq!(scc.explain_keep_from.as_deref(), Some("line "));
        let tokei = &adapters[2];
        assert!(tokei.explain_args.is_none());
        assert!(
            tokei
                .format_explain_command(&tokei.invocations[0], Path::new("t.exe"), Path::new("a.py"))
                .is_none()
        );

        let path = write_an_adapter(
            "an_explain_command_with_no_file",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\n\
             explain-args = [\"-t\"]\n\
             [dialect.default]\nargs = []\n",
        );
        let refused = Adapter::read(&path, &read_the_shipped_dialects()).unwrap_err();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        assert!(refused.contains("no {file} in its explain-args"), "{refused}");
    }

    #[test]
    fn the_format_an_analysis_is_read_as_needs_a_command_and_rules_out_trimming_it_as_text() {
        let dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters")];
        let adapters = Adapter::read_all(&dirs, &read_the_shipped_dialects()).unwrap();
        let mezura = &adapters[0];
        assert_eq!(mezura.explain_output, Some(PerLineFormat::LinejudgePerLine));
        assert_eq!(mezura.explain_keep_from, None);
        assert_eq!(adapters[1].explain_output, None);

        let alone = write_an_adapter(
            "a_format_with_no_command",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\n\
             explain-output = \"linejudge-per-line\"\n\
             [dialect.default]\nargs = []\n",
        );
        let refused = Adapter::read(&alone, &read_the_shipped_dialects()).unwrap_err();
        fs::remove_dir_all(alone.parent().unwrap()).unwrap();
        assert!(refused.contains("no explain-args to print it"), "{refused}");

        let both = write_an_adapter(
            "a_format_and_a_trim",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\n\
             explain-args = [\"-t\", \"{file}\"]\nexplain-output = \"linejudge-per-line\"\n\
             explain-keep-from = \"line \"\n\
             [dialect.default]\nargs = []\n",
        );
        let refused = Adapter::read(&both, &read_the_shipped_dialects()).unwrap_err();
        fs::remove_dir_all(both.parent().unwrap()).unwrap();
        assert!(refused.contains("two ways of taking the same output"), "{refused}");

        let unknown = write_an_adapter(
            "a_format_nobody_reads",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\n\
             explain-args = [\"-t\", \"{file}\"]\nexplain-output = \"tokei-lines\"\n\
             [dialect.default]\nargs = []\n",
        );
        let refused = Adapter::read(&unknown, &read_the_shipped_dialects()).unwrap_err();
        fs::remove_dir_all(unknown.parent().unwrap()).unwrap();
        assert!(refused.contains("does not parse"), "{refused}");
    }

    #[test]
    fn a_keep_from_needs_a_command_to_trim_and_may_not_be_empty() {
        let alone = write_an_adapter(
            "a_keep_from_with_no_command",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\n\
             explain-keep-from = \"line \"\n\
             [dialect.default]\nargs = []\n",
        );
        let refused = Adapter::read(&alone, &read_the_shipped_dialects()).unwrap_err();
        fs::remove_dir_all(alone.parent().unwrap()).unwrap();
        assert!(refused.contains("no explain-args to trim"), "{refused}");

        let empty = write_an_adapter(
            "an_empty_keep_from",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\n\
             explain-args = [\"-t\", \"{file}\"]\nexplain-keep-from = \"\"\n\
             [dialect.default]\nargs = []\n",
        );
        let refused = Adapter::read(&empty, &read_the_shipped_dialects()).unwrap_err();
        fs::remove_dir_all(empty.parent().unwrap()).unwrap();
        assert!(refused.contains("keeps everything"), "{refused}");
    }

    #[test]
    fn a_version_nobody_answers_is_unknown_and_stops_nothing() {
        let dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters")];
        let tokei = &Adapter::read_all(&dirs, &read_the_shipped_dialects()).unwrap()[2];
        assert_eq!(tokei.version_flag.as_deref(), Some("--version"));
        assert_eq!(tokei.read_version_or_unknown(Path::new("a-binary-that-does-not-exist")), "unknown version");

        let path = write_an_adapter(
            "an_adapter_with_no_version_flag",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\nversion-flag = \"\"\n\
             [dialect.default]\nargs = []\n",
        );
        let unversioned = Adapter::read(&path, &read_the_shipped_dialects()).unwrap();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        assert_eq!(unversioned.version_flag, None);
        assert_eq!(unversioned.read_version_or_unknown(Path::new("irrelevant")), "unknown version");
    }

    #[test]
    fn a_declared_version_is_recognised_inside_whatever_the_counter_prints_around_it() {
        let says = is_the_declared_version;
        assert!(says("14.0.0", "tokei 14.0.0 compiled with serialization support: json"));
        assert!(says("3.7.0", "scc version 3.7.0"));
        assert!(!says("14.0.0", "tokei 13.0.0 compiled with serialization support: json"));
        // Both ends of the match matter: one number can sit inside another from either side.
        assert!(!says("4.0.0", "tokei 14.0.0 compiled with serialization support: json"));
        assert!(!says("3.7", "scc version 3.7.1"));
    }

    #[test]
    fn an_adapter_that_names_no_way_of_counting_is_refused() {
        let path = write_an_adapter(
            "an_adapter_with_no_dialect",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\n\
             [dialect]\n",
        );
        let refused = Adapter::read(&path, &read_the_shipped_dialects()).unwrap_err();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        assert!(refused.contains("no way of counting"), "{refused}");
    }

    #[test]
    fn the_file_named_among_one_dialects_own_arguments_is_refused() {
        let path = write_an_adapter(
            "an_adapter_naming_the_file_twice",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\n\
             [dialect.default]\nargs = [\"{file}\"]\n",
        );
        let refused = Adapter::read(&path, &read_the_shipped_dialects()).unwrap_err();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        assert!(refused.contains("the arguments every dialect shares"), "{refused}");
    }

    #[test]
    fn an_adapter_that_never_names_the_file_is_refused() {
        let path = write_an_adapter(
            "an_adapter_with_no_file",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"--output\", \"json\"]\n\
             [dialect.default]\nargs = []\n",
        );
        let refused = Adapter::read(&path, &read_the_shipped_dialects()).unwrap_err();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
        assert!(refused.contains("{file}"), "{refused}");
    }

    #[test]
    fn an_adapter_naming_a_dialect_with_no_buckets_is_refused() {
        let path = write_an_adapter(
            "an_adapter_with_an_unknown_dialect",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\n\
             [dialect.strict]\nargs = []\n",
        );
        let refused = Adapter::read(&path, &read_the_shipped_dialects()).unwrap_err();
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
