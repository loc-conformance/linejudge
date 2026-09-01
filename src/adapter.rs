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

/// Every channel a counter can be downloaded from, for a refusal that names them all.
pub const CHANNELS: [&str; 3] = [CRATES_IO, GITHUB_RELEASE_ASSET, GITHUB_RELEASE_FILE];

/// The channel that compiles a counter from its crates.io source.
pub const CRATES_IO: &str = "crates-io";

/// The channel that picks a release asset by the system and architecture in its name.
pub const GITHUB_RELEASE_ASSET: &str = "github-release-asset";

/// The channel that takes a release file named outright in `[acquisition.file]`.
pub const GITHUB_RELEASE_FILE: &str = "github-release-file";

/// The `[acquisition.file]` key for any system the table does not name.
pub const OTHER_SYSTEM: &str = "other";

/// Stands for the acquisition's version inside a release file name.
pub const VERSION_PLACEHOLDER: &str = "{version}";

/// The directory the explain scripts are read from, one per counter that needs one.
pub const EXPLAIN_SCRIPTS_DIR: &str = "explain-scripts";

/// What stands in the version's place for a counter that declares no version flag, and for one
/// whose flag was asked and answered nothing.
pub const UNKNOWN_VERSION: &str = "unknown version";

const ADAPTER_EXTENSION: &str = "toml";
const BINARY_PLACEHOLDER: &str = "{binary}";
const FILE_PLACEHOLDER: &str = "{file}";
const EXPLAIN_SCRIPTS_PLACEHOLDER: &str = "{explain-scripts}";
const RELEASE_FILE_SYSTEMS: [&str; 4] = ["windows", "linux", "macos", OTHER_SYSTEM];
const VERSION_FLAG: &str = "--version";

/// Everything needed to run one counter and read its answer, as `adapters/<counter>.toml`
/// declares it.
#[derive(Debug)]
pub struct Adapter {
    /// The counter this runs, which is also what its file is named after.
    pub name_of_counter: String,
    /// Where the counter itself lives, for a report that links to it. `None` is not linked.
    pub repository: Option<String>,
    /// The command line it is run with, `{file}` standing for the file, and the arguments of the
    /// chosen way of counting appended after these.
    pub args: Vec<String>,
    /// The command line that asks for a reading of a file line by line, used in place of `args`.
    /// `None` is a counter with no such command, which is not a failure.
    pub explain_args: Option<Vec<String>>,
    /// The program those arguments go to, for a counter that reads no file line by line itself
    /// and has a wrapper doing it. `None` sends them to the counter's own binary, and then
    /// `{binary}` and `{explain-scripts}` mean nothing and are left as they are written.
    pub explain_command: Option<String>,
    /// The format that reading is printed in, declared together with `explain-args` or not at
    /// all: a reading nobody could compare would not be asked for.
    pub explain_output: Option<PerLineFormat>,
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
        if raw.explain_output.is_some() && raw.explain_args.is_none() {
            return Err(format!(
                "{} declares explain-output with no explain-args to print it",
                path.display()
            ));
        }
        if raw.explain_args.is_some() && raw.explain_output.is_none() {
            return Err(format!(
                "{} declares explain-args with no explain-output to read what they print",
                path.display()
            ));
        }
        if raw.explain_command.is_some() && raw.explain_args.is_none() {
            return Err(format!(
                "{} declares explain-command with no explain-args to give it",
                path.display()
            ));
        }
        if raw.explain_command.is_none()
            && let Some(explain) = &raw.explain_args
            && let Some(misplaced) =
                explain.iter().find(|a| a.contains(BINARY_PLACEHOLDER) || a.contains(EXPLAIN_SCRIPTS_PLACEHOLDER))
        {
            return Err(format!(
                "{} writes {misplaced} with no explain-command, and those two stand for something \
                 only in the arguments of a wrapper",
                path.display()
            ));
        }
        // Whether a channel is one this build can download from is asked by the two commands that
        // download, since every other command reads an adapter without ever looking at the block.
        if let Some(how) = &raw.acquisition {
            if how.channel == GITHUB_RELEASE_FILE && how.file.is_none() {
                return Err(format!(
                    "{}: {GITHUB_RELEASE_FILE} needs an [acquisition.file] table saying which \
                     release file to take",
                    path.display()
                ));
            }
            if how.channel != GITHUB_RELEASE_FILE && how.file.is_some() {
                return Err(format!(
                    "{}: an [acquisition.file] table belongs to {GITHUB_RELEASE_FILE}, and {} \
                     picks its file itself",
                    path.display(),
                    how.channel
                ));
            }
            for (system, named) in how.file.iter().flatten() {
                if !RELEASE_FILE_SYSTEMS.contains(&system.as_str()) {
                    return Err(format!(
                        "{}: {system} is not a system a release file can be named for: it knows {}",
                        path.display(),
                        RELEASE_FILE_SYSTEMS.join(", ")
                    ));
                }
                if !named.contains(VERSION_PLACEHOLDER) && named.contains(&how.version) {
                    return Err(format!(
                        "{}: {named} writes the version out and goes stale the day it is raised, \
                         so say {VERSION_PLACEHOLDER}",
                        path.display()
                    ));
                }
            }
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
            explain_command: raw.explain_command,
            explain_output: raw.explain_output,
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
        self.read_version(binary).unwrap_or_else(|_| UNKNOWN_VERSION.to_string())
    }

    /// What the binary answers to its version flag. `Err` is a binary that could not be run at
    /// all, which is a different trouble from one that runs and names a version nobody asked for.
    pub fn read_version(&self, binary: &Path) -> Result<String, String> {
        let Some(flag) = &self.version_flag else { return Ok(UNKNOWN_VERSION.to_string()) };
        let printed = run_counter(binary, std::slice::from_ref(flag))?;
        let trimmed = printed.trim();
        Ok(match trimmed.is_empty() {
            true => UNKNOWN_VERSION.to_string(),
            false => trimmed.to_string(),
        })
    }

    /// Runs the per-line command and hands back what it printed, as it printed it. `None` is an
    /// adapter that declares no such command.
    pub fn run_explain(
        &self,
        invocation: &Invocation,
        binary: &Path,
        file: &Path,
        scripts: &[PathBuf],
    ) -> Option<Result<String, String>> {
        let base = self.explain_args.as_ref()?;
        // A wrapper named by `explain-command` takes the counter as `{binary}`; otherwise the
        // counter is run directly.
        let (program, args) = match &self.explain_command {
            Some(program) => (
                PathBuf::from(program),
                build_wrapper_args(base, invocation, binary, file, scripts),
            ),
            None => (binary.to_path_buf(), build_command_args(base, invocation, file)),
        };
        Some(run_counter(&program, &args))
    }

    /// The command as a person would retype it, meant to be pasted into a shell. Paths under the
    /// directory the run works from are written relative to it and the rest are left whole.
    pub fn format_command(&self, invocation: &Invocation, binary: &Path, file: &Path) -> String {
        let args = self.build_args(invocation, &shorten_the_path(file));
        format!("{} {}", shorten(binary), args.join(" "))
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
    /// for one that publishes only source and has to be compiled, `github-release-file` for one
    /// whose release files say nothing about systems and are named in `file` below instead.
    pub channel: String,
    /// What the channel calls it: the `owner/repo` on GitHub, or the name of the crate.
    pub name: String,
    /// Downloaded exactly, never resolved to whatever is newest, so two runs on different days
    /// measure the same build.
    pub version: String,
    /// The release file to take, named outright per system: `windows`, `linux`, `macos`, and
    /// `other` for any system not named. `{version}` in a name stands for the version above.
    pub file: Option<BTreeMap<String, String>>,
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

// A wrapper is handed the counter it speaks for and the directory it was itself written to, since
// it is run from wherever the person running this happened to be. The directories are layered like
// every other, so `{explain-scripts}` becomes the last of them that holds the script named after it, and
// the one this build carries where none does, which is what makes the refusal name a real path.
fn build_wrapper_args(
    base: &[String],
    invocation: &Invocation,
    binary: &Path,
    file: &Path,
    scripts: &[PathBuf],
) -> Vec<String> {
    build_command_args(base, invocation, file)
        .iter()
        .map(|a| a.replace(BINARY_PLACEHOLDER, &binary.display().to_string()))
        .map(|a| fill_in_the_scripts_dir(&a, scripts))
        .collect()
}

fn fill_in_the_scripts_dir(arg: &str, scripts: &[PathBuf]) -> String {
    let Some((_, after)) = arg.split_once(EXPLAIN_SCRIPTS_PLACEHOLDER) else {
        return arg.to_string();
    };
    // The script is what the placeholder is followed by, so the winning layer is the last one that
    // holds that file, not the last whose whole substituted argument happens to be a file. The two
    // differ the moment the placeholder sits inside a longer token, `--script={explain-scripts}/x`.
    let script = after.trim_start_matches(['/', '\\']);
    let fill = |dir: &PathBuf| arg.replace(EXPLAIN_SCRIPTS_PLACEHOLDER, &dir.display().to_string());
    scripts
        .iter()
        .rev()
        .find(|dir| dir.join(script).is_file())
        .map(&fill)
        .or_else(|| scripts.first().map(&fill))
        .unwrap_or_else(|| arg.to_string())
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
    #[serde(rename = "explain-command")]
    explain_command: Option<String>,
    #[serde(rename = "explain-output")]
    explain_output: Option<PerLineFormat>,
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
        assert!(!adapters.is_empty(), "no adapter was read");
        let names: Vec<&str> = adapters.iter().map(|a| a.name_of_counter.as_str()).collect();
        let mut in_order = names.clone();
        in_order.sort_unstable();
        assert_eq!(names, in_order, "the roster came back out of order");
        for adapter in &adapters {
            let who = &adapter.name_of_counter;
            assert!(!adapter.invocations.is_empty(), "{who} declares no way of counting");
            for way in &adapter.invocations {
                assert!(!way.buckets.is_empty(), "{who}.{} names no bucket", way.name);
            }
            let home = adapter.repository.as_deref().unwrap_or_default();
            assert!(home.starts_with("https://"), "{who}: {home}");
            if let Some(how) = &adapter.acquisition {
                assert!(CHANNELS.contains(&how.channel.as_str()), "{who}: {}", how.channel);
                assert!(!how.version.is_empty(), "{who} names no version to fetch");
            }
        }
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
        let missing = Adapter::read_one(&dirs, "sloccount", &dialects).unwrap_err();
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

    // The layered directory holds the script, so it is the one named. This is what lets somebody
    // measuring their own counter keep its wrapper in their own repository.
    #[test]
    fn a_wrapper_is_taken_from_the_last_directory_that_holds_it() {
        let path = write_an_adapter(
            "an_adapter_bringing_a_wrapper",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\n\
             explain-command = \"perl\"\n\
             explain-args = [\"{explain-scripts}/probe.pl\", \"{binary}\", \"{file}\"]\n\
             explain-output = \"linejudge-per-line\"\n\
             [dialect.default]\nargs = []\n",
        );
        let adapter = Adapter::read(&path, &read_the_shipped_dialects()).unwrap();
        fs::remove_dir_all(path.parent().unwrap()).unwrap();

        let carried = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(EXPLAIN_SCRIPTS_DIR);
        let theirs = env::temp_dir().join("linejudge-a_wrapper_of_their_own");
        let _ = fs::remove_dir_all(&theirs);
        fs::create_dir_all(&theirs).unwrap();
        fs::write(theirs.join("probe.pl"), "not really a wrapper").unwrap();

        let asked = |scripts: &[PathBuf]| {
            build_wrapper_args(adapter.explain_args.as_ref().unwrap(), &adapter.invocations[0],
                    Path::new("t.exe"), Path::new("a.py"), scripts)
        };
        let layered = asked(&[carried.clone(), theirs.clone()]);
        // A directory holding no such script is passed over, and the first layer answers.
        let empty = env::temp_dir().join("linejudge-a_scripts_dir_with_nothing_in_it");
        let _ = fs::remove_dir_all(&empty);
        fs::create_dir_all(&empty).unwrap();
        let missing = asked(&[carried, empty.clone()]);
        fs::remove_dir_all(&theirs).unwrap();
        fs::remove_dir_all(&empty).unwrap();

        assert!(layered[0].contains("a_wrapper_of_their_own"), "{layered:?}");
        assert_eq!(layered[1..], ["t.exe".to_string(), "a.py".to_string()]);
        assert!(missing[0].replace('\\', "/").ends_with("explain-scripts/probe.pl"), "{missing:?}");
    }

    #[test]
    fn a_placeholder_inside_a_longer_token_still_finds_the_script() {
        let theirs = env::temp_dir().join("linejudge-embedded_placeholder_theirs");
        let empty = env::temp_dir().join("linejudge-embedded_placeholder_empty");
        for dir in [&theirs, &empty] {
            let _ = fs::remove_dir_all(dir);
            fs::create_dir_all(dir).unwrap();
        }
        fs::write(theirs.join("cloc.pl"), "not really a wrapper").unwrap();
        // The placeholder is glued onto a flag, so the script is what follows it. The directory
        // that holds cloc.pl wins, not the one whose whole substituted argument is a file, which
        // none is once "--script=" sits in front.
        let filled =
            fill_in_the_scripts_dir("--script={explain-scripts}/cloc.pl", &[empty.clone(), theirs.clone()]);
        fs::remove_dir_all(&theirs).unwrap();
        fs::remove_dir_all(&empty).unwrap();
        assert!(filled.starts_with("--script="), "{filled}");
        assert!(filled.contains("theirs"), "{filled}");
        assert!(!filled.contains("empty"), "{filled}");
    }

    #[test]
    fn every_acquisition_mistake_is_refused_when_the_adapter_is_read() {
        let base = "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\n\
                    [dialect.default]\nargs = []\n\
                    [acquisition]\nchannel = \"github-release-file\"\nname = \"a/b\"\n\
                    version = \"2.10\"\n";
        let read = |dir: &str, text: &str| {
            let path = write_an_adapter(dir, text);
            let outcome = Adapter::read(&path, &read_the_shipped_dialects());
            fs::remove_dir_all(path.parent().unwrap()).unwrap();
            outcome
        };
        let table = "[acquisition.file]\nother = \"t-{version}.pl\"\n";

        // A channel this build cannot download from is refused by the commands that download,
        // and reading the file is not where that question belongs.
        let unknown = read("a_channel_nobody_knows",
                &base.replace("github-release-file", "gitlab-release"));
        assert_eq!(unknown.unwrap().acquisition.unwrap().channel, "gitlab-release");

        let bare = read("a_release_file_channel_with_no_table", base);
        assert!(bare.unwrap_err().contains("needs an [acquisition.file] table"));

        let mispaired = read("a_file_table_on_another_channel",
                &format!("{}{table}", base.replace("github-release-file", "crates-io")));
        assert!(mispaired.unwrap_err().contains("belongs to github-release-file"));

        let crooked = read("a_system_nobody_ships_for",
                &format!("{base}[acquisition.file]\nosx = \"t-{{version}}.pl\"\n"));
        assert!(crooked.unwrap_err().contains("osx is not a system"));

        let stale = read("a_file_name_with_the_version_written_out",
                &format!("{base}[acquisition.file]\nother = \"t-2.10.pl\"\n"));
        assert!(stale.unwrap_err().contains("say {version}"));

        // The version can sit in a name for its own reasons, and only a name without the
        // placeholder is the one that goes stale.
        let both = read("a_name_holding_the_version_and_the_placeholder",
                &format!("{base}[acquisition.file]\nother = \"t2.10-{{version}}.pl\"\n"));
        assert!(both.is_ok(), "{:?}", both.unwrap_err());

        let sound = read("an_acquisition_with_nothing_wrong", &format!("{base}{table}"));
        let how = sound.unwrap().acquisition.unwrap();
        assert_eq!(how.file.unwrap()["other"], "t-{version}.pl");
    }

    #[test]
    fn the_file_takes_the_place_of_its_placeholder_and_the_dialect_speaks_last() {
        let dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters")];
        let mezura = &Adapter::read_all(&dirs, &read_the_shipped_dialects()).unwrap()[1];
        let args = mezura.build_args(&mezura.invocations[1], Path::new("a/case/input.rs"));
        assert_eq!(args[0], "a/case/input.rs");
        assert_eq!(args[args.len() - 2..], ["--counting".to_string(), "region".to_string()]);
    }

    #[test]
    fn the_per_line_command_is_declared_per_counter_and_has_to_name_the_file() {
        let dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters")];
        let adapters = Adapter::read_all(&dirs, &read_the_shipped_dialects()).unwrap();
        let scc = &adapters[2];
        let scripts = [PathBuf::from(EXPLAIN_SCRIPTS_DIR)];
        let scripts = scripts.as_slice();
        assert_eq!(
            scc.explain_args.as_deref().unwrap(),
            ["-t", "--no-cocomo", "--format", "json", "{file}"]
        );
        assert_eq!(scc.explain_output, Some(PerLineFormat::SccTrace));

        let cloc = &adapters[0];
        assert_eq!(cloc.explain_args.as_deref().unwrap(), ["--print-filter-stages", "--json", "{file}"]);
        assert_eq!(cloc.explain_output, Some(PerLineFormat::ClocStages));

        let tokei = &adapters[3];
        assert!(tokei.explain_args.is_none());
        assert!(
            tokei
                .run_explain(&tokei.invocations[0], Path::new("t.exe"), Path::new("a.py"), scripts)
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
    fn explain_args_and_explain_output_are_declared_together_or_not_at_all() {
        let dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters")];
        let adapters = Adapter::read_all(&dirs, &read_the_shipped_dialects()).unwrap();
        assert_eq!(adapters[1].explain_output, Some(PerLineFormat::LinejudgePerLine));
        assert_eq!(adapters[3].explain_output, None);

        let alone = write_an_adapter(
            "a_format_with_no_command",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\n\
             explain-output = \"linejudge-per-line\"\n\
             [dialect.default]\nargs = []\n",
        );
        let refused = Adapter::read(&alone, &read_the_shipped_dialects()).unwrap_err();
        fs::remove_dir_all(alone.parent().unwrap()).unwrap();
        assert!(refused.contains("no explain-args to print it"), "{refused}");

        let unreadable = write_an_adapter(
            "a_command_with_no_format",
            "name = \"tokei\"\noutput = \"tokei-json\"\nargs = [\"{file}\"]\n\
             explain-args = [\"-t\", \"{file}\"]\n\
             [dialect.default]\nargs = []\n",
        );
        let refused = Adapter::read(&unreadable, &read_the_shipped_dialects()).unwrap_err();
        fs::remove_dir_all(unreadable.parent().unwrap()).unwrap();
        assert!(refused.contains("no explain-output to read what they print"), "{refused}");
    }

    #[test]
    fn a_version_nobody_answers_is_unknown_and_stops_nothing() {
        let dirs = vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters")];
        let tokei = &Adapter::read_all(&dirs, &read_the_shipped_dialects()).unwrap()[3];
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
