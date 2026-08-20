#![forbid(unsafe_code)]

mod explain;
mod marks;
#[cfg(feature = "maintenance")]
mod record;
mod render;
mod report;
mod style;

use std::env;
use std::io::{self, ErrorKind, IsTerminal, Write};
use std::path::{self, Path, PathBuf};
use std::process::{self, ExitCode};
use std::vec::IntoIter;

use linejudge::adapter::Adapter;
use linejudge::corpus::Corpus;
use linejudge::counters::Counters;
use linejudge::dialects::Dialects;
use linejudge::fetched;
use linejudge::known_failures::KnownFailures;
use linejudge::linejudge_folder::COUNTERS_FILE;
use linejudge::linejudge_folder::Folder;
use linejudge::recorded::RECORDED_DIR;
use linejudge::recorded::{RecordedAnswers, is_same_build};
use linejudge::shipped::create_the_shipped_dir;
use linejudge::verdict::measure_and_judge_every_case;

use crate::explain::{explain_one_counter, find_case};
use crate::report::OneRun;
use crate::report::{
    report_entries_that_name_nothing, report_recorded_answers_that_name_nothing,
    report_the_verdicts_of_one_dialect,
};

const ADAPTERS_DIR: &str = "adapters";
const CASES_DIR: &str = "cases";
const DIALECTS_DIR: &str = "dialects";
const SITE_DIR: &str = "site";
const USAGE: &str = "\
linejudge check [<case>] [--counter <name>] [--bin <path>] [--known-failures <file>]
                [--corpus <dir>] [--adapters <dir>] [--dialects <dir>] [--recorded <dir>]
                [--disabled <case>]

    Runs every counter it has a binary for over every case, and answers two questions apart. The
    first is conformance: does the counter answer what its own declared rules ask for, judged for
    any counter at all. The second is drift: does it still answer what it did when its answers
    were recorded, judged only where recorded/<counter>.toml holds a photograph and the binary is
    the version written at the top of it; a different build is said once and what changed since
    is reported, never judged. A case a counter breaks on, by exiting non-zero or printing
    something unreadable, is an outcome of its own beside the failures, and every other case is
    measured anyway.

    The cases, the adapters, the dialects and the recorded answers are carried inside this binary
    and need nothing else on disk. --corpus replaces the corpus; --adapters, --dialects and
    --recorded are layered over what is carried, per counter, so a directory naming one counter
    declares that one and leaves every other as it is.

    Binaries are named in .linejudge/counters.toml, or with --bin, which needs --counter to say
    whose binary it is, and whatever a fetch has downloaded is found without anybody naming a
    path. The .linejudge folder is looked for upward from the working directory,
    the way cargo finds its own, and its settings.toml can name the corpus, adapters, dialects,
    recorded and known-failures paths, each meaning what the flag of the same name means. A flag
    wins over the folder.

    Naming a case judges that one and nothing else, and the exit code is then its own. It is named
    the way this report names it, or by any part of the name that fits exactly one case, so
    'check 2150' is enough and the run says which case that was.

    A case whose directory name starts with 'disabled-' is set aside and named, never judged,
    since the prefix says this suite's own resolution of it is not to be trusted. --disabled sets
    one more aside for a single run.

    With --known-failures, the run breaks on a failing case the file does not name, and on nothing
    else. One case per line, named the way this report names it, '#' starts a comment, and
    'region:8010-punctuation_only_line' names one way of counting where naming the case alone
    names them all. It needs --counter.

linejudge explain <case> [--counter <name>] [--bin <path>] [--corpus <dir>] [--adapters <dir>]
                [--dialects <dir>]

    Prints, for one case, how each way of counting reads every line of it: the marked spans, the
    rule that took the line and the predicates that hold on it, and under those whatever per-line
    analysis the counter itself can print, run through the explain-args of its adapter. A case is
    named the way check names it, or by any part of the name that fits exactly one case, and no
    binary is needed for anything but the counter's own analysis.

    A counter whose analysis is the linejudge-per-line format is read rather than shown, and every
    line it reads differently from these rules is named where that line is.

linejudge render [--out <dir>] [--corpus <dir>] [--adapters <dir>] [--dialects <dir>]
                [--recorded <dir>]

    Measures every counter it has a binary for over every case, the way check does, and writes the
    published pages instead of a report: index.html, the scoreboard of every counter over every
    case with the failures explained on hover, a page under cases/ for each of them holding the
    file with its marked spans and every answer to it, and data.json, the whole measurement as one
    record for anybody building their own view of the same numbers. They land in --out, or in
    ./site. A counter with no binary is named on stderr and left out rather than failing the run.

    The scoreboard is opened afterwards with whatever this machine opens an HTML file with, unless
    the output is not a terminal, so a run on a build machine writes the pages and opens nothing.

The commands print colour when they print to a terminal. NO_COLOR turns it off wherever it is set,
and CLICOLOR_FORCE keeps it through a pipe.
";
#[cfg(feature = "maintenance")]
const RECORD_USAGE: &str = "
linejudge record --counter <name> [--bin <path>] [--corpus <dir>] [--adapters <dir>]
                [--dialects <dir>] [--recorded <dir>]

    Writes recorded/<name>.toml from scratch: runs that counter over every case, reads the version
    out of its binary, and records what it answered. It is how this suite keeps its own photographs
    current and it is not something a consumer of the corpus needs, so it is built only with
    --features maintenance.

    A note is kept exactly as long as the answer it was written about and dropped the moment that
    answer moves, and every note dropped is named, since the sentence is owed by a person. An
    exception is carried over as it stands. A counter that breaks on any case is refused rather
    than written down with a hole in it.
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
            eprintln!("{}", style::DIFFERS.paint(&trouble.to_string()));
            ExitCode::FAILURE
        }
        Err(Trouble::Said(message)) => {
            eprintln!("{}", style::DIFFERS.paint(&message));
            ExitCode::FAILURE
        }
    }
}

/// A command the build does not carry is a command nobody is told about, so the help text of a
/// plain build names only what a plain build can run.
fn get_the_usage() -> String {
    #[cfg(not(feature = "maintenance"))]
    return USAGE.to_string();
    #[cfg(feature = "maintenance")]
    format!("{USAGE}{RECORD_USAGE}")
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
    let mut settings = Settings::of(args)?;
    let mut out = io::stdout().lock();
    if settings.wants_help {
        writeln!(out, "{}", get_the_usage())?;
        return Ok(false);
    }
    let folder = match env::current_dir() {
        Ok(here) => Folder::find(&here)?,
        Err(_) => None,
    };
    // The run moves to the folder's root, so that a relative path inside an adapter's args, a
    // wrapper script say, resolves against the project wherever the command was typed, by the same
    // rule the counters file already follows. A path given as a flag means it from where it was
    // typed, so those are pinned down first.
    if let Some(folder) = &folder {
        anchor_every_path_of(&mut settings);
        env::set_current_dir(folder.get_root()).map_err(|e| {
            format!("{} could not be moved into: {e}", folder.get_root().display())
        })?;
    }
    let shipped = create_the_shipped_dir()?;
    let dirs = resolve_dirs(&settings, folder.as_ref(), &shipped)?;
    let dialects = Dialects::read(&dirs.dialects).map_err(|faults| faults.join("\n"))?;
    let mut corpus = read_the_corpus(&dirs.corpus)?;
    set_aside_what_was_disabled(&mut corpus, &settings.disabled)?;
    let adapters = match &settings.name_of_counter {
        Some(name) => vec![Adapter::read_one(&dirs.adapters, name, &dialects)?],
        None => Adapter::read_all(&dirs.adapters, &dialects)?,
    };
    // With --bin there is no reason to open the counters file, the desired counter tool has been
    // named.
    let counters = match (&settings.name_of_counter, &settings.binary) {
        (Some(counter), Some(binary)) => {
            let mut counters = Counters::empty();
            counters.name_binary(counter, binary.clone());
            counters
        }
        _ => match &folder {
            Some(folder) => {
                let mut counters = Counters::read(&folder.get_counters_file())?;
                counters.resolve_against(folder.get_root());
                counters
            }
            None => Counters::empty(),
        },
    };
    let find_binary = |counter: &str| {
        counters.find_binary(counter).map(Path::to_path_buf).or_else(|| {
            adapters
                .iter()
                .find(|adapter| adapter.name_of_counter == counter)
                .and_then(|adapter| adapter.acquisition.as_ref())
                .and_then(|how| fetched::find_the_binary_of(counter, &how.version))
        })
    };
    let known_failures = match (&dirs.known_failures, &settings.name_of_counter) {
        (Some(path), Some(_)) => Some(KnownFailures::read(path)?),
        (Some(_), None) => {
            return Err(Trouble::Said(
                "a known-failures list needs --counter to say whose failures it names".to_string(),
            ));
        }
        (None, _) => None,
    };

    // Naming one case narrows the corpus itself, so everything below judges and reports exactly
    // that case and the exit code is its own.
    let one_case = matches!(&settings.command, Command::Check { case } if !case.is_empty());
    if let Command::Check { case } = &settings.command
        && one_case
    {
        let named = find_the_case_named(&mut out, &corpus, case, "judge")?;
        corpus.cases.retain(|one| one.name == named);
    }

    if let Command::Explain { case } = &settings.command {
        let named = find_the_case_named(&mut out, &corpus, case, "explain")?;
        let found = find_case(&corpus, &named).map_err(Trouble::Said)?;
        for adapter in &adapters {
            let binary = find_binary(&adapter.name_of_counter);
            explain_one_counter(&mut out, adapter, binary.as_deref(), found, &dialects, &corpus.readings)?;
        }
        return Ok(false);
    }

    if let Command::Render = &settings.command {
        let site = settings.out.clone().unwrap_or_else(|| PathBuf::from(SITE_DIR));
        let cases = render::write_the_site(
            &adapters,
            &corpus,
            &dialects,
            &dirs.recorded,
            &find_binary,
            &site,
        )?;
        let index = site.join(render::INDEX_FILE);
        writeln!(
            out,
            "wrote {}, {} and a page for each of {cases} cases",
            index.display(),
            site.join(render::DATA_FILE).display()
        )?;
        if io::stdout().is_terminal() {
            open_the_page(&index);
        }
        return Ok(false);
    }

    if !corpus.disabled.is_empty() && !one_case {
        let what = if corpus.disabled.len() == 1 { "case is" } else { "cases are" };
        writeln!(out, "{}", style::RECORDED.paint(&format!(
                "{} {what} set aside as disabled and not judged: {}",
                corpus.disabled.len(), corpus.disabled.join(", "))))?;
    }
    #[cfg(feature = "maintenance")]
    if let Command::Record = &settings.command {
        let name = &adapters[0].name_of_counter;
        let Some(binary) = find_binary(name) else {
            return Err(Trouble::Said(format!("{name}: no binary named for it, nothing to record")));
        };
        let into = dirs.recorded.last().filter(|dir| !dir.starts_with(&shipped));
        let Some(into) = into else {
            return Err(Trouble::Said(
                "record writes into a recorded directory of your own, and none was named"
                    .to_string(),
            ));
        };
        let held = RecordedAnswers::read(&dirs.recorded, name, &dialects)
            .map_err(|faults| faults.join("\n"))?;
        record::record_one_counter(
            &mut out,
            &adapters[0],
            &binary,
            &corpus,
            &dialects,
            held.as_ref(),
            into,
        )?;
        return Ok(false);
    }

    let mut broken = false;
    let mut ran = 0;
    for adapter in &adapters {
        let name = &adapter.name_of_counter;
        let Some(binary) = find_binary(name) else {
            writeln!(out, "{}", style::RECORDED.paint(&format!(
                    "{name}: no binary named for it, nothing run")))?;
            continue;
        };
        ran += 1;
        let version = adapter.read_version_or_unknown(&binary);
        let record = RecordedAnswers::read(&dirs.recorded, name, &dialects)
            .map_err(|faults| faults.join("\n"))?;
        let drift_is_judged =
            record.as_ref().is_some_and(|record| is_same_build(&record.version, &version));
        if let Some(record) = &record
            && !drift_is_judged
        {
            writeln!(out, "\n{}  {}", style::HEADING.paint(name), style::RECORDED.paint(&format!(
                    "recorded at [{}] and running [{version}], so what changed since the record \
                     is not judged", record.version)))?;
        }
        for dialect in &adapter.dialects {
            let Some(rules) = dialects.find(name, &dialect.name) else {
                return Err(Trouble::Said(format!(
                    "{name}.{} names no dialect file to judge by", dialect.name
                )));
            };
            let judged = measure_and_judge_every_case(
                adapter,
                dialect,
                rules,
                &binary,
                &corpus,
                record.as_ref(),
                &version,
            )
            .map_err(|faults| faults.join("\n"))?;
            let run = OneRun { adapter, dialect, binary: &binary, version: &version, drift_is_judged };
            broken |=
                report_the_verdicts_of_one_dialect(&mut out, &run, &judged, known_failures.as_ref())?;
        }
        // Both of these hold a list against the whole corpus, and the corpus is one case here, so
        // asking about one case would report every other as named by a list and missing.
        if one_case {
            continue;
        }
        if let Some(record) = &record {
            report_recorded_answers_that_name_nothing(&mut out, record, &corpus)?;
        }
        if let Some(known_failures) = &known_failures {
            report_entries_that_name_nothing(&mut out, adapter, &corpus, known_failures)?;
        }
    }
    // A run that counted nothing and said everything was fine is the one failure a green build
    // hides, and a name misspelled in the counters file is all it takes.
    if ran == 0 {
        return Err(Trouble::Said(format!(
            "no counter was run: name a binary with --bin, or in .linejudge/{COUNTERS_FILE} beside \
             the project"
        )));
    }
    Ok(broken)
}

#[derive(Debug)]
struct Settings {
    command: Command,
    corpus: Option<PathBuf>,
    adapters: Option<PathBuf>,
    dialects: Option<PathBuf>,
    recorded: Option<PathBuf>,
    name_of_counter: Option<String>,
    binary: Option<PathBuf>,
    known_failures: Option<PathBuf>,
    disabled: Vec<String>,
    out: Option<PathBuf>,
    wants_help: bool,
}

#[derive(Debug)]
enum Command {
    /// An empty name is every case, which is the ordinary run.
    Check { case: String },
    Explain { case: String },
    Render,
    #[cfg(feature = "maintenance")]
    Record,
}

impl Settings {
    fn of(args: Vec<String>) -> Result<Settings, String> {
        let mut settings = Settings {
            command: Command::Check { case: String::new() },
            corpus: None,
            adapters: None,
            dialects: None,
            recorded: None,
            name_of_counter: None,
            binary: None,
            known_failures: None,
            disabled: Vec::new(),
            out: None,
            wants_help: false,
        };
        let mut args = args.into_iter();
        let Some(command) = args.next() else { return Err(get_the_usage()) };
        match command.as_str() {
            "--help" | "-h" => {
                settings.wants_help = true;
                return Ok(settings);
            }
            "check" => {}
            "explain" => settings.command = Command::Explain { case: String::new() },
            "render" => settings.command = Command::Render,
            #[cfg(feature = "maintenance")]
            "record" => settings.command = Command::Record,
            _ => {
                let usage = get_the_usage();
                return Err(format!("{command} is not a command of this program\n\n{usage}"));
            }
        }
        // The flag is recognised before its value is taken, so a misspelled last flag is told it is
        // misspelled instead of being told it was given nothing.
        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--help" | "-h" => settings.wants_help = true,
                "--corpus" => settings.corpus = Some(PathBuf::from(value_of(&flag, &mut args)?)),
                "--adapters" => settings.adapters = Some(PathBuf::from(value_of(&flag, &mut args)?)),
                "--dialects" => settings.dialects = Some(PathBuf::from(value_of(&flag, &mut args)?)),
                "--recorded" => settings.recorded = Some(PathBuf::from(value_of(&flag, &mut args)?)),
                "--counter" => settings.name_of_counter = Some(value_of(&flag, &mut args)?),
                "--bin" => settings.binary = Some(PathBuf::from(value_of(&flag, &mut args)?)),
                "--known-failures" => {
                    settings.known_failures = Some(PathBuf::from(value_of(&flag, &mut args)?))
                }
                "--disabled" => settings.disabled.push(value_of(&flag, &mut args)?),
                "--out" => settings.out = Some(PathBuf::from(value_of(&flag, &mut args)?)),
                _ => match &mut settings.command {
                    Command::Check { case } | Command::Explain { case }
                        if case.is_empty() && !flag.starts_with('-') =>
                    {
                        *case = flag;
                    }
                    _ => {
                        let usage = get_the_usage();
                        return Err(format!("{flag} is not a flag of this command\n\n{usage}"));
                    }
                },
            }
        }
        if settings.wants_help {
            return Ok(settings);
        }
        if let Command::Explain { case } = &settings.command {
            if case.is_empty() {
                return Err("explain needs the name of a case".to_string());
            }
            if settings.known_failures.is_some() {
                return Err("--known-failures belongs to check".to_string());
            }
            if !settings.disabled.is_empty() {
                return Err("--disabled belongs to check".to_string());
            }
            if settings.recorded.is_some() {
                return Err("--recorded belongs to check".to_string());
            }
            if settings.out.is_some() {
                return Err("--out belongs to render".to_string());
            }
        }
        if matches!(settings.command, Command::Check { .. }) && settings.out.is_some() {
            return Err("--out belongs to render".to_string());
        }
        if let Command::Render = &settings.command {
            if settings.name_of_counter.is_some() || settings.binary.is_some() {
                return Err(
                    "render measures the whole roster, so --counter and --bin do not belong to it"
                        .to_string(),
                );
            }
            if settings.known_failures.is_some() {
                return Err("--known-failures belongs to check".to_string());
            }
            if !settings.disabled.is_empty() {
                return Err("--disabled belongs to check, and a page with a case left out of it \
                            is not the page"
                    .to_string());
            }
        }
        #[cfg(feature = "maintenance")]
        if let Command::Record = &settings.command {
            if settings.name_of_counter.is_none() {
                return Err("record needs --counter to say whose answers it writes".to_string());
            }
            if settings.known_failures.is_some() {
                return Err("--known-failures belongs to check".to_string());
            }
            if !settings.disabled.is_empty() {
                return Err("--disabled belongs to check, and a record with a case left out of it \
                            is not a record"
                    .to_string());
            }
            if settings.out.is_some() {
                return Err("--out belongs to render".to_string());
            }
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

/// A corpus is replaced whole, since half of one corpus beside half of another is neither. The
/// other three are layered over what this build carries, the last directory winning per counter.
struct Dirs {
    corpus: PathBuf,
    adapters: Vec<PathBuf>,
    dialects: Vec<PathBuf>,
    recorded: Vec<PathBuf>,
    known_failures: Option<PathBuf>,
}

fn resolve_dirs(settings: &Settings, folder: Option<&Folder>, shipped: &Path) -> Result<Dirs, String> {
    let corpus = settings.corpus.clone().or_else(|| folder.and_then(Folder::find_corpus));
    let adapters = settings.adapters.clone().or_else(|| folder.and_then(Folder::find_adapters));
    let dialects = settings.dialects.clone().or_else(|| folder.and_then(Folder::find_dialects));
    let recorded = settings.recorded.clone().or_else(|| folder.and_then(Folder::find_recorded));
    for dir in [&corpus, &adapters, &dialects, &recorded].into_iter().flatten() {
        if !dir.is_dir() {
            return Err(format!("{} is not a directory, so nothing can be read there", dir.display()));
        }
    }
    let layer = |under: &str, named: Option<PathBuf>| {
        let mut dirs = vec![shipped.join(under)];
        dirs.extend(named);
        dirs
    };
    Ok(Dirs {
        // A record is the photograph of one corpus, so a corpus of somebody else's leaves the
        // carried records out rather than judging against answers to cases nobody loaded.
        recorded: match corpus.is_some() {
            true => recorded.into_iter().collect(),
            false => layer(RECORDED_DIR, recorded),
        },
        corpus: corpus.unwrap_or_else(|| shipped.join(CASES_DIR)),
        adapters: layer(ADAPTERS_DIR, adapters),
        dialects: layer(DIALECTS_DIR, dialects),
        known_failures: settings
            .known_failures
            .clone()
            .or_else(|| folder.and_then(Folder::find_known_failures)),
    })
}

/// Hands the page to whatever the machine opens an HTML file with. Nothing is checked and nothing
/// is waited for: the pages are written either way, and a machine with no browser at all is not a
/// run that failed.
fn open_the_page(page: &Path) {
    let mut opener = if cfg!(target_os = "windows") {
        // The empty argument is the window title, which start takes from the first quoted one.
        let mut shell = process::Command::new("cmd");
        shell.args(["/C", "start", ""]);
        shell
    } else if cfg!(target_os = "macos") {
        process::Command::new("open")
    } else {
        process::Command::new("xdg-open")
    };
    let _ = opener.arg(page).spawn();
}

/// `check` and `explain` name a case the same way, so they refuse an unknown one the same way and
/// both say out loud which case a fragment turned out to name.
fn find_the_case_named(
    out: &mut dyn Write,
    corpus: &Corpus,
    name: &str,
    what: &str,
) -> Result<String, Trouble> {
    let found = match find_case(corpus, name) {
        Ok(found) => found.name.clone(),
        Err(_) if corpus.disabled.iter().any(|one| one.contains(name)) => {
            return Err(Trouble::Said(format!(
                "{name} names a disabled case, whose resolution this suite itself does not trust, \
                 so there is nothing honest to {what}"
            )));
        }
        Err(message) => return Err(Trouble::Said(message)),
    };
    if found != name {
        writeln!(out, "{}", style::DETAIL.paint(&format!(
                "no case is named {name}, so this is {found}")))?;
    }
    Ok(found)
}

fn anchor_every_path_of(settings: &mut Settings) {
    for path in [
        &mut settings.corpus,
        &mut settings.adapters,
        &mut settings.dialects,
        &mut settings.recorded,
        &mut settings.binary,
        &mut settings.known_failures,
        &mut settings.out,
    ]
    .into_iter()
    .flatten()
    {
        *path = path::absolute(&path).unwrap_or_else(|_| path.clone());
    }
}

fn value_of(flag: &str, args: &mut IntoIter<String>) -> Result<String, String> {
    args.next().ok_or_else(|| format!("{flag} was given nothing"))
}

fn read_the_corpus(dir: &Path) -> Result<Corpus, String> {
    let corpus = Corpus::read(dir).map_err(|faults| {
        let what = if faults.len() == 1 { "fault" } else { "faults" };
        let mut report = format!("the cases could not be read, {} {what}:", faults.len());
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

fn set_aside_what_was_disabled(corpus: &mut Corpus, named: &[String]) -> Result<(), String> {
    for name in named {
        match corpus.cases.iter().position(|case| case.name == *name) {
            Some(at) => {
                corpus.cases.remove(at);
                corpus.disabled.push(name.clone());
            }
            None if corpus.disabled.contains(name) => {}
            None => {
                return Err(format!("--disabled names {name}, and there is no case of that name"));
            }
        }
    }
    corpus.disabled.sort();
    Ok(())
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

    #[test]
    fn check_takes_one_case_by_name_and_no_name_is_the_whole_corpus() {
        let parsed = settings_of(&["check", "2150", "--counter", "tokei"]).unwrap();
        match parsed.command {
            Command::Check { case } => assert_eq!(case, "2150"),
            _ => panic!("check parsed as another command"),
        }
        match settings_of(&["check", "--counter", "tokei"]).unwrap().command {
            Command::Check { case } => assert!(case.is_empty()),
            _ => panic!("check parsed as another command"),
        }
        let second = settings_of(&["check", "one-case", "another-case"]).unwrap_err();
        assert!(second.contains("not a flag"), "{second}");
    }

    #[test]
    fn explain_takes_one_case_by_name_and_the_check_only_flags_are_refused_on_it() {
        let parsed = settings_of(&["explain", "0400-a_case", "--counter", "scc"]).unwrap();
        match parsed.command {
            Command::Explain { case } => assert_eq!(case, "0400-a_case"),
            _ => panic!("explain parsed as another command"),
        }
        let empty = settings_of(&["explain"]).unwrap_err();
        assert!(empty.contains("needs the name of a case"), "{empty}");
        let second = settings_of(&["explain", "one-case", "another-case"]).unwrap_err();
        assert!(second.contains("not a flag"), "{second}");
        for check_only in [
            ["explain", "one-case", "--known-failures", "known.txt"],
            ["explain", "one-case", "--disabled", "one-case"],
            ["explain", "one-case", "--recorded", "recorded"],
        ] {
            assert!(settings_of(&check_only).unwrap_err().contains("belongs to check"));
        }
    }

    #[test]
    fn a_named_corpus_replaces_the_carried_one_and_a_named_directory_layers_over_the_rest() {
        let root = std::env::temp_dir().join("linejudge-directories_a_test_names");
        let mine = root.join("mine");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&mine).unwrap();
        let carried = Path::new("what-this-build-carries");
        let of = |args: &[&str]| resolve_dirs(&settings_of(args).unwrap(), None, carried);

        let named_corpus = of(&["check", "--corpus", &mine.display().to_string()]).unwrap();
        let layered = of(&["check", "--adapters", &mine.display().to_string()]).unwrap();
        let missing = of(&["check", "--dialects", &root.join("nope").display().to_string()]);
        std::fs::remove_dir_all(&root).unwrap();

        assert_eq!(named_corpus.corpus, mine);
        assert_eq!(named_corpus.adapters, [carried.join(ADAPTERS_DIR)]);
        assert!(named_corpus.recorded.is_empty(), "{:?}", named_corpus.recorded);
        assert_eq!(named_corpus.known_failures, None);

        assert_eq!(layered.corpus, carried.join(CASES_DIR));
        assert_eq!(layered.adapters, [carried.join(ADAPTERS_DIR), mine]);
        assert_eq!(layered.dialects, [carried.join(DIALECTS_DIR)]);
        assert_eq!(layered.recorded, [carried.join(RECORDED_DIR)]);

        let refused = match missing {
            Ok(_) => panic!("a directory that is not there was taken anyway"),
            Err(refused) => refused,
        };
        assert!(refused.contains("is not a directory"), "{refused}");
    }

    #[test]
    fn render_measures_the_whole_roster_and_the_narrowing_flags_are_refused_on_it() {
        assert!(settings_of(&["render"]).is_ok());
        assert!(settings_of(&["render", "--out", "pages"]).unwrap().out.is_some());
        assert!(settings_of(&["render", "--corpus", "cases"]).is_ok());
        for narrowed in [
            vec!["render", "--counter", "scc"],
            vec!["render", "--counter", "scc", "--bin", "scc.exe"],
        ] {
            assert!(settings_of(&narrowed).unwrap_err().contains("whole roster"));
        }
        let with_list = settings_of(&["render", "--known-failures", "known.txt"]);
        assert!(with_list.unwrap_err().contains("belongs to check"));
        let one_out = settings_of(&["render", "--disabled", "0400-a_case"]);
        assert!(one_out.unwrap_err().contains("is not the page"));
        let on_check = settings_of(&["check", "--out", "pages"]);
        assert!(on_check.unwrap_err().contains("belongs to render"));
    }

    #[test]
    fn a_disabled_flag_names_a_case_or_is_refused() {
        let parsed = settings_of(&["check", "--disabled", "0400-a", "--disabled", "0500-b"]).unwrap();
        assert_eq!(parsed.disabled, ["0400-a", "0500-b"]);
    }

    fn settings_of(args: &[&str]) -> Result<Settings, String> {
        Settings::of(args.iter().map(|a| a.to_string()).collect())
    }
}
