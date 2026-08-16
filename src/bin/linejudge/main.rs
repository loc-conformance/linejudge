#![forbid(unsafe_code)]

mod report;

use std::env;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::vec::IntoIter;

use linejudge::adapter::Adapter;
use linejudge::corpus::Corpus;
use linejudge::counters::COUNTERS_FILE;
use linejudge::counters::Counters;
use linejudge::known_failures::KnownFailures;
use linejudge::verdict::measure_and_judge_every_case;

use crate::report::{report_entries_that_name_nothing, report_the_verdicts_of_one_dialect};

const ADAPTERS_DIR: &str = "adapters";
const CASES_DIR: &str = "cases";
const USAGE: &str = "\
linejudge check [--counter <name>] [--bin <path>] [--known-failures <file>] [--corpus <dir>]
                [--adapters <dir>]

    Runs every counter it has a binary for over every case, and says for each of them whether it
    answers what its own rules ask for. Binaries are named in linejudge-counters.toml beside this
    command, or with --bin, which needs --counter to say whose binary it is.

    With --known-failures, the run breaks on a failing case the file does not name, and on nothing
    else. One case per line, by number, '#' starts a comment, and 'region:2400' names one dialect
    of the counter where naming the case alone would name them all. It needs --counter.
";

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(broken) if broken => ExitCode::FAILURE,
        Ok(_) => ExitCode::SUCCESS,
        // A report cut short by 'linejudge check | head' is not a run that failed, and complaining
        // about it would mean writing into the pipe that has just gone away.
        Err(Trouble::Writing(trouble)) if trouble.kind() == ErrorKind::BrokenPipe => {
            ExitCode::SUCCESS
        }
        Err(Trouble::Writing(trouble)) => {
            eprintln!("{trouble}");
            ExitCode::FAILURE
        }
        Err(Trouble::Said(message)) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

enum Trouble {
    Said(String),
    Writing(io::Error),
}

impl From<String> for Trouble {
    fn from(message: String) -> Trouble {
        Trouble::Said(message)
    }
}

impl From<io::Error> for Trouble {
    fn from(trouble: io::Error) -> Trouble {
        Trouble::Writing(trouble)
    }
}

fn run(args: Vec<String>) -> Result<bool, Trouble> {
    let settings = Settings::of(args)?;
    let mut out = io::stdout().lock();
    if settings.wants_help {
        writeln!(out, "{USAGE}")?;
        return Ok(false);
    }
    let corpus = read_the_corpus(&settings.corpus)?;
    let adapters = match &settings.name_of_counter {
        Some(name) => vec![Adapter::read_one(&settings.adapters, name)?],
        None => Adapter::read_all(&settings.adapters)?,
    };
    // With --bin there is no reason to open linejudge-counters.toml, the desired
    // counter tool has been named
    let counters = match (&settings.name_of_counter, &settings.binary) {
        (Some(counter), Some(binary)) => {
            let mut counters = Counters::empty();
            counters.name_binary(counter, binary.clone());
            counters
        }
        _ => Counters::read(Path::new(COUNTERS_FILE))?,
    };
    let known_failures = match &settings.known_failures {
        Some(path) => Some(KnownFailures::read(path)?),
        None => None,
    };

    let mut broken = false;
    let mut ran = 0;
    for adapter in &adapters {
        let Some(binary) = counters.find_binary(&adapter.name_of_counter) else {
            writeln!(out, "{}: no binary named for it, nothing run", adapter.name_of_counter)?;
            continue;
        };
        ran += 1;
        let version = adapter.read_version_or_unknown(binary);
        for dialect in &adapter.dialects {
            let judged = measure_and_judge_every_case(adapter, dialect, binary, &corpus)?;
            broken |= report_the_verdicts_of_one_dialect(
                &mut out,
                adapter,
                dialect,
                binary,
                &version,
                &judged,
                known_failures.as_ref(),
            )?;
        }
        if let Some(known_failures) = &known_failures {
            report_entries_that_name_nothing(&mut out, adapter, &corpus, known_failures)?;
        }
    }
    // A run that counted nothing and said everything was fine is the one failure a green build
    // hides, and a name misspelled in the counters file is all it takes.
    if ran == 0 {
        return Err(Trouble::Said(format!(
            "no counter was run: name a binary with --bin, or in {COUNTERS_FILE} beside the command"
        )));
    }
    Ok(broken)
}

#[derive(Debug)]
struct Settings {
    corpus: PathBuf,
    adapters: PathBuf,
    name_of_counter: Option<String>,
    binary: Option<PathBuf>,
    known_failures: Option<PathBuf>,
    wants_help: bool,
}

impl Settings {
    fn of(args: Vec<String>) -> Result<Settings, String> {
        let mut settings = Settings {
            corpus: PathBuf::from(CASES_DIR),
            adapters: PathBuf::from(ADAPTERS_DIR),
            name_of_counter: None,
            binary: None,
            known_failures: None,
            wants_help: false,
        };
        let mut args = args.into_iter();
        let Some(command) = args.next() else { return Err(USAGE.to_string()) };
        if command == "--help" || command == "-h" {
            settings.wants_help = true;
            return Ok(settings);
        }
        if command != "check" {
            return Err(format!("{command} is not a command of this program\n\n{USAGE}"));
        }
        // The flag is recognised before its value is taken, so a misspelled last flag is told it is
        // misspelled instead of being told it was given nothing.
        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--help" | "-h" => settings.wants_help = true,
                "--corpus" => settings.corpus = PathBuf::from(value_of(&flag, &mut args)?),
                "--adapters" => settings.adapters = PathBuf::from(value_of(&flag, &mut args)?),
                "--counter" => settings.name_of_counter = Some(value_of(&flag, &mut args)?),
                "--bin" => settings.binary = Some(PathBuf::from(value_of(&flag, &mut args)?)),
                "--known-failures" => {
                    settings.known_failures = Some(PathBuf::from(value_of(&flag, &mut args)?))
                }
                _ => return Err(format!("{flag} is not a flag of this command\n\n{USAGE}")),
            }
        }
        if settings.wants_help {
            return Ok(settings);
        }
        if settings.binary.is_some() && settings.name_of_counter.is_none() {
            return Err("--bin needs --counter to say whose binary it is".to_string());
        }
        if settings.known_failures.is_some() && settings.name_of_counter.is_none() {
            return Err("--known-failures needs --counter to say whose failures it names".to_string());
        }
        Ok(settings)
    }
}

fn value_of(flag: &str, args: &mut IntoIter<String>) -> Result<String, String> {
    args.next().ok_or_else(|| format!("{flag} was given nothing"))
}

fn read_the_corpus(dir: &Path) -> Result<Corpus, String> {
    let corpus = Corpus::read(dir).map_err(|faults| {
        let mut report = format!("{} cases could not be read:", faults.len());
        for fault in &faults {
            report.push_str(&format!("\n  {fault}"));
        }
        report
    })?;
    if corpus.cases.is_empty() {
        return Err(format!("{} holds no case at all", dir.display()));
    }
    Ok(corpus)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_binary_and_a_list_of_failures_both_need_the_counter_they_belong_to() {
        assert!(settings_of(&["check", "--bin", "tokei.exe"]).unwrap_err().contains("--counter"));
        let no_counter = settings_of(&["check", "--known-failures", "known.txt"]);
        assert!(no_counter.unwrap_err().contains("--counter"));
        assert!(settings_of(&["check", "--counter", "tokei", "--bin", "tokei.exe"]).is_ok());
    }

    #[test]
    fn a_flag_with_nothing_after_it_is_refused_and_so_is_an_unknown_one() {
        assert!(settings_of(&["check", "--counter"]).unwrap_err().contains("was given nothing"));
        assert!(settings_of(&["check", "--fast", "yes"]).unwrap_err().contains("not a flag"));
        assert!(settings_of(&["measure"]).unwrap_err().contains("not a command"));
        assert!(settings_of(&[]).unwrap_err().contains("linejudge check"));
    }

    #[test]
    fn a_misspelled_last_flag_is_told_what_is_wrong_with_it() {
        let wrong = settings_of(&["check", "--knwon-failures"]).unwrap_err();
        assert!(wrong.contains("not a flag"), "{wrong}");
    }

    #[test]
    fn help_is_asked_for_in_every_position_and_is_not_an_error() {
        for args in [vec!["--help"], vec!["check", "--help"], vec!["check", "-h"]] {
            assert!(settings_of(&args).unwrap().wants_help);
        }
    }

    fn settings_of(args: &[&str]) -> Result<Settings, String> {
        Settings::of(args.iter().map(|a| a.to_string()).collect())
    }
}
