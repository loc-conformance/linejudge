#![forbid(unsafe_code)]

#[cfg(feature = "maintenance")]
mod bump;
mod counters;
mod explain;
mod fetch;
mod fetched;
mod linejudge_folder;
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

use linejudge::adapter::{ADAPTERS_DIR, Adapter};
use linejudge::corpus::{CASES_DIR, Corpus};
use linejudge::dialects::{DIALECTS_DIR, Dialects};
use linejudge::known_failures::KnownFailures;
use linejudge::recorded::RECORDED_DIR;
use linejudge::recorded::{RecordedAnswers, is_same_build};
use linejudge::shipped::create_the_shipped_dir;
use linejudge::verdict::measure_and_judge_every_case;

use crate::counters::Counters;
use crate::explain::{explain_one_counter, find_case};
use crate::linejudge_folder::{COUNTERS_FILE, Folder};
use crate::report::OneRun;
use crate::report::{
    report_entries_that_name_nothing, report_recorded_answers_that_name_nothing,
    report_the_verdicts_of_one_dialect,
};

const COMMAND_OPENS: &str = "linejudge ";
const SITE_DIR: &str = "site";
const USAGE: &str = "\
linejudge check [<case>] [--counter <name>] [--bin <path>] [--known-failures <file>]
                [--corpus <dir>] [--adapters <dir>] [--dialects <dir>] [--recorded <dir>]
                [--disabled <case>]

    Runs each counter over every case and asks two things of every answer: does it match what that
    counter's own rules say, and does it match what the counter answered when it was last recorded.
    A counter that crashes on a case is reported apart from one that answers wrongly, and the
    remaining cases still run.

    Name a case to run only that one. Any part of the name is enough if it fits exactly one case.

    --counter <name>         run this counter and no other
    --bin <path>             the binary to run it with; needs --counter
    --known-failures <file>  fail only on cases this file does not list; needs --counter
    --disabled <case>        leave one more case out of this run
    --corpus <dir>           use these cases instead of the built-in ones
    --adapters <dir>         replace the built-in adapters for the counters named inside
    --dialects <dir>         replace the built-in dialects for the counters named inside
    --recorded <dir>         replace the built-in records for the counters named inside

    Binaries are found in .linejudge/counters.toml, or given with --bin, or downloaded by fetch.
    linejudge looks for .linejudge here and in every directory above, the way cargo does. Its
    settings.toml can hold the four directory paths and the known-failures path, and a flag on the
    command line beats it.

    A known-failures file holds one case per line and '#' starts a comment. '8010-blank_line'
    allows that case for every way the counter counts; 'region:8010-blank_line' allows it for the
    'region' way only.

linejudge explain <case> [--counter <name>] [--bin <path>] [--corpus <dir>] [--adapters <dir>]
                [--dialects <dir>]

    Prints one case line by line: the strings and comments marked in it, the rule that decided each
    line, and what the counter itself says about the same line. Any part of the case name is enough
    if it fits exactly one case.

    --counter <name>         explain this counter and no other
    --bin <path>             the binary to run it with; needed only for what the counter says

    --corpus, --adapters and --dialects mean what they mean in check.

linejudge fetch [<counter>] [--adapters <dir>] [--dialects <dir>]

    Downloads the counters this suite knows about and puts each one where the other commands find
    it, so no path has to be given afterwards. Name a counter to fetch only that one.

    Each is downloaded at the exact version its adapter file declares. What arrives is asked its
    version and thrown away when the answer is not that version, so a download that quietly hands
    over something else is never measured. A counter that fails does not stop the others, and a
    path in counters.toml still wins over anything fetch downloaded.

linejudge render [--out <dir>] [--corpus <dir>] [--adapters <dir>] [--dialects <dir>]
                [--recorded <dir>]

    Measures the way check does and writes web pages instead of a report: the scoreboard, a page
    per case, and data.json holding the whole measurement.

    --out <dir>              where to write them; ./site by default

    --corpus, --adapters, --dialects and --recorded mean what they mean in check.

    The scoreboard opens in a browser when the output is a terminal, so a build machine writes the
    pages and opens nothing. A counter with no binary is left out and named on stderr.

Output to a terminal has colour and output to a file or a pipe does not. Set NO_COLOR to turn it
off, or CLICOLOR_FORCE to keep it through a pipe.
";
#[cfg(feature = "maintenance")]
const RECORD_USAGE: &str = "
linejudge record --counter <name> [--bin <path>] [--corpus <dir>] [--adapters <dir>]
                [--dialects <dir>] [--recorded <dir>]

    Writes recorded/<name>.toml from scratch: runs that counter over every case and records what
    it answered, at the version its binary printed. It maintains this suite's own record and no
    consumer of the corpus needs it, so it is built only with --features maintenance.

    A note is kept exactly as long as the answer it was written about, and every note dropped is
    named, since the sentence is owed by a person. A counter that breaks on any case is refused
    rather than written down with a hole in it.
";
#[cfg(feature = "maintenance")]
const BUMP_USAGE: &str = "
linejudge bump-versions [<counter>] [--json] [--adapters <dir>]

    Asks every channel what it publishes newest and, where that differs from the version an
    adapter declares, writes the new one into it and changes nothing else in the file. Named a
    counter, it does that for that one. It maintains this suite's own declarations and no consumer
    of the corpus needs it, so it is built only with --features maintenance.

    The comparison is equality alone: a channel answering something unexpected reads as a
    difference rather than passing quietly, and no order between two version numbers is invented.

    A raised version is half a change. The recorded answers of that counter were measured against
    the old build, and moving them is the other half.

    --json prints the counters that moved as one document and nothing else, with anything gone
    wrong on the error output, which is what a scheduled job builds its work list from.
";

fn main() -> ExitCode {
    match run(env::args().skip(1).collect()) {
        Ok(broken) if broken => ExitCode::FAILURE,
        Ok(_) => ExitCode::SUCCESS,
        // A report cut short by `linejudge check | head` is not a run that failed, and complaining
        // would mean writing into the pipe that just went away.
        Err(Trouble::Writing(trouble)) if trouble.kind() == ErrorKind::BrokenPipe => {
            ExitCode::SUCCESS
        }
        Err(Trouble::Writing(trouble)) => {
            eprintln!("{}", style::DIFFERS.paint(&trouble.to_string()));
            ExitCode::FAILURE
        }
        // What went wrong is painted; anything under a blank line is the usage or a list of
        // faults printed as it stands, since painting it red reads as more of the complaint.
        Err(Trouble::Said(message)) => {
            match message.split_once("\n\n") {
                Some((said, under)) => {
                    eprintln!("{}\n\n{}", style::DIFFERS.paint(said), paint_the_usage(under));
                }
                None => eprintln!("{}", style::DIFFERS.paint(&message)),
            }
            ExitCode::FAILURE
        }
    }
}

// The help of a plain build names only what a plain build can run.
fn get_the_usage() -> String {
    #[cfg(not(feature = "maintenance"))]
    return USAGE.to_string();
    #[cfg(feature = "maintenance")]
    format!("{USAGE}{RECORD_USAGE}{BUMP_USAGE}")
}

// A flag that does not exist is a mistake inside one command, so that command's own block answers
// it and the other three are not printed. Blocks are told apart by where they start: a command
// opens at the first column and everything belonging to it is indented or blank.
fn find_the_usage_of(named: &str) -> String {
    let whole = get_the_usage();
    let opening = format!("{COMMAND_OPENS}{named} ");
    let mut block: Vec<&str> = Vec::new();
    for line in whole.lines().skip_while(|line| !line.starts_with(&opening)) {
        if !block.is_empty() && !line.is_empty() && !line.starts_with(' ') {
            break;
        }
        block.push(line);
    }
    match block.is_empty() {
        true => whole,
        false => format!("{}\n", block.join("\n").trim_end()),
    }
}

// Painted here and never where the help is built, so what the parser hands back stays plain text.
fn paint_the_usage(text: &str) -> String {
    let mut painted: Vec<String> = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        if line.starts_with(COMMAND_OPENS) {
            inside = true;
            painted.push(paint_a_command_line(line));
            continue;
        }
        if line.trim().is_empty() {
            inside = false;
        }
        painted.push(match (inside, line.is_empty()) {
            (true, _) => paint_the_arguments(line, style::PLAIN),
            // Painting an empty line writes escape codes around nothing.
            (false, true) => line.to_string(),
            (false, false) => paint_the_arguments(line, style::FADED),
        });
    }
    painted.join("\n")
}

fn paint_a_command_line(line: &str) -> String {
    let mut words = line.splitn(3, ' ');
    let (Some(program), Some(named)) = (words.next(), words.next()) else {
        return line.to_string();
    };
    let painted = format!("{program} {}", style::COMMAND.paint(named));
    match words.next() {
        Some(rest) => format!("{painted} {}", paint_the_arguments(rest, style::PLAIN)),
        None => painted,
    }
}

// A flag is painted wherever it appears, on a command line and in the table under it alike. `rest`
// is what everything around the flags is painted with, which is the only thing that differs.
fn paint_the_arguments(text: &str, rest: style::Style) -> String {
    cut_the_arguments(text)
        .iter()
        .map(|(ink, piece)| match *ink == style::FLAG {
            true => ink.paint(piece).to_string(),
            false => rest.paint(piece).to_string(),
        })
        .collect()
}

// Walked by character because a flag is glued to the bracket in front of it, so splitting on
// spaces would hand back `[--counter`.
fn cut_the_arguments(text: &str) -> Vec<(style::Style, String)> {
    let mut cut: Vec<(style::Style, String)> = Vec::new();
    let mut plain = String::new();
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '-' || chars.peek() != Some(&'-') {
            plain.push(ch);
            continue;
        }
        let mut held = String::from(ch);
        while chars.peek().is_some_and(|ch| !" ]".contains(*ch)) {
            held.extend(chars.next());
        }
        if !plain.is_empty() {
            cut.push((style::PLAIN, std::mem::take(&mut plain)));
        }
        cut.push((style::FLAG, held));
    }
    if !plain.is_empty() {
        cut.push((style::PLAIN, plain));
    }
    cut
}

// Nothing says which command a misspelled one was meant to be, so the answer is the list of them.
fn name_every_command() -> String {
    let whole = get_the_usage();
    let mut named: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in whole.lines() {
        if line.starts_with(COMMAND_OPENS) {
            if !named.is_empty() {
                named.push("");
            }
            inside = true;
        } else if line.trim().is_empty() {
            inside = false;
        }
        if inside {
            named.push(line);
        }
    }
    named.join("\n")
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
    if let Some(help) = &settings.help {
        writeln!(out, "{}", paint_the_usage(help))?;
        return Ok(false);
    }
    let folder = match env::current_dir() {
        Ok(here) => Folder::find(&here)?,
        Err(_) => None,
    };
    // The run moves to the folder's root, so a relative path inside an adapter's args resolves
    // against the project wherever the command was typed. A path given as a flag means it from
    // where it was typed, so those are pinned down first.
    if let Some(folder) = &folder {
        anchor_every_path_of(&mut settings);
        env::set_current_dir(folder.get_root()).map_err(|e| {
            format!("{} could not be moved into: {e}", folder.get_root().display())
        })?;
    }
    let shipped = create_the_shipped_dir()?;
    let dirs = resolve_dirs(&settings, folder.as_ref(), &shipped)?;
    let dialects = Dialects::read(&dirs.dialects).map_err(|faults| faults.join("\n"))?;
    // Downloading asks nothing of the corpus or of anybody's answers to it, so neither is read.
    if let Command::Fetch { counter } = &settings.command {
        let adapters = Adapter::read_all(&dirs.adapters, &dialects)?;
        let chosen = choose_the_counters_named(&mut out, &adapters, counter)?;
        let named = read_the_counters_of(folder.as_ref())?;
        return Ok(fetch::fetch_every_counter(&mut out, &chosen, &named)?);
    }
    // Asking a channel what it publishes touches neither the corpus nor anybody's answers either.
    #[cfg(feature = "maintenance")]
    if let Command::BumpVersions { counter } = &settings.command {
        let adapters = Adapter::read_all(&dirs.adapters, &dialects)?;
        let chosen = choose_the_counters_named(&mut out, &adapters, counter)?;
        let into = dirs.adapters.last().filter(|dir| !dir.starts_with(&shipped));
        let Some(into) = into else {
            return Err(Trouble::Said(
                "bump-versions writes into an adapters directory of your own, and none was named"
                    .to_string(),
            ));
        };
        return Ok(bump::bump_every_version(&mut out, &chosen, into, settings.as_json)?);
    }
    let mut corpus = read_the_corpus(&dirs.corpus)?;
    set_aside_what_was_disabled(&mut corpus, &settings.disabled)?;
    let adapters = match &settings.name_of_counter {
        Some(name) => vec![Adapter::read_one(&dirs.adapters, name, &dialects)?],
        None => Adapter::read_all(&dirs.adapters, &dialects)?,
    };
    // With --bin the binary is already named, so the counters file is not opened at all.
    let counters = match (&settings.name_of_counter, &settings.binary) {
        (Some(counter), Some(binary)) => {
            let mut counters = Counters::empty();
            counters.name_binary(counter, binary.clone());
            counters
        }
        _ => read_the_counters_of(folder.as_ref())?,
    };
    let find_binary = |name_of_counter: &str| {
        counters.find_binary(name_of_counter).map(Path::to_path_buf).or_else(|| {
            adapters
                .iter()
                .find(|adapter| adapter.name_of_counter == name_of_counter)
                .and_then(|adapter| adapter.acquisition.as_ref())
                .and_then(|how| fetched::find_the_binary_of(name_of_counter, &how.version))
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

    // Naming one case narrows the corpus itself, so everything below judges that case alone.
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
        for dialect in &adapter.invocations {
            let judged = measure_and_judge_every_case(
                adapter,
                dialect,
                &dialects,
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
        // Both hold a list against the whole corpus, and the corpus is one case here, so asking
        // would report every other case as named by a list and missing.
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
    // A run that counted nothing and reported success is the one failure a green build hides, and
    // a name misspelled in the counters file is all it takes.
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
    #[cfg(feature = "maintenance")]
    as_json: bool,
    // The help to print instead of running, and `None` for a run. `linejudge --help` asks for the
    // whole of it; a `--help` after a command asks about that command and gets that block.
    help: Option<String>,
}

#[derive(Debug)]
enum Command {
    // An empty name is every case, which is the ordinary run.
    Check { case: String },
    Explain { case: String },
    // An empty name is every counter that says where it comes from.
    Fetch { counter: String },
    Render,
    #[cfg(feature = "maintenance")]
    Record,
    // An empty name asks every channel and writes nothing.
    #[cfg(feature = "maintenance")]
    BumpVersions { counter: String },
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
            #[cfg(feature = "maintenance")]
            as_json: false,
            help: None,
        };
        let mut args = args.into_iter();
        let Some(command) = args.next() else { return Err(get_the_usage()) };
        match command.as_str() {
            "--help" | "-h" => {
                settings.help = Some(get_the_usage());
                return Ok(settings);
            }
            "check" => {}
            "explain" => settings.command = Command::Explain { case: String::new() },
            "fetch" => settings.command = Command::Fetch { counter: String::new() },
            "render" => settings.command = Command::Render,
            #[cfg(feature = "maintenance")]
            "record" => settings.command = Command::Record,
            #[cfg(feature = "maintenance")]
            "bump-versions" => {
                settings.command = Command::BumpVersions { counter: String::new() }
            }
            _ => {
                let named = name_every_command();
                return Err(format!("{command} is not a command of this program\n\n{named}\n"));
            }
        }
        // The flag is recognised before its value is taken, so a misspelled last flag is told it
        // is misspelled instead of being told it was given nothing.
        while let Some(flag) = args.next() {
            match flag.as_str() {
                "--help" | "-h" => settings.help = Some(find_the_usage_of(&command)),
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
                #[cfg(feature = "maintenance")]
                "--json" => settings.as_json = true,
                _ => match &mut settings.command {
                    Command::Check { case }
                    | Command::Explain { case }
                    | Command::Fetch { counter: case }
                        if case.is_empty() && !flag.starts_with('-') =>
                    {
                        *case = flag;
                    }
                    #[cfg(feature = "maintenance")]
                    Command::BumpVersions { counter }
                        if counter.is_empty() && !flag.starts_with('-') =>
                    {
                        *counter = flag;
                    }
                    _ => {
                        let usage = find_the_usage_of(&command);
                        return Err(format!("{flag} is not a flag of this command\n\n{usage}"));
                    }
                },
            }
        }
        if settings.help.is_some() {
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
        #[cfg(feature = "maintenance")]
        if settings.as_json && !matches!(settings.command, Command::BumpVersions { .. }) {
            return Err("--json belongs to bump-versions".to_string());
        }
        if let Command::Fetch { .. } = &settings.command {
            if settings.name_of_counter.is_some() {
                return Err("fetch names the counter on its own, without --counter".to_string());
            }
            for (named, whose) in [
                (settings.binary.is_some(), "--bin"),
                (settings.corpus.is_some(), "--corpus"),
                (settings.recorded.is_some(), "--recorded"),
                (settings.known_failures.is_some(), "--known-failures"),
                (!settings.disabled.is_empty(), "--disabled"),
                (settings.out.is_some(), "--out"),
            ] {
                if named {
                    return Err(format!("fetch downloads binaries, so {whose} is nothing to it"));
                }
            }
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

// A corpus is replaced whole, since half of one beside half of another is neither. The other three
// are layered over what this build carries, the last directory winning per counter.
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
        // A record answers one corpus, so somebody else's corpus leaves the carried records out
        // rather than judging against answers to cases nobody loaded.
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

// Nothing is checked and nothing is waited for: the pages are written either way, and a machine
// with no browser is not a run that failed.
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

// `check` and `explain` name a case the same way, so they refuse an unknown one the same way and
// both say which case a fragment turned out to name.
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

// Each path is resolved against the directory holding `.linejudge`, so a committed one works from
// any subdirectory. No folder means nobody named anything.
fn read_the_counters_of(folder: Option<&Folder>) -> Result<Counters, String> {
    let Some(folder) = folder else { return Ok(Counters::empty()) };
    let mut counters = Counters::read(&folder.get_counters_file())?;
    counters.resolve_against(folder.get_root());
    Ok(counters)
}

// No name is every counter. A name is matched the way a case is: exactly, or by any part of it
// that fits exactly one, so `fetch tok` is enough and the run says which counter that was.
fn choose_the_counters_named<'a>(
    out: &mut dyn Write,
    adapters: &'a [Adapter],
    name: &str,
) -> Result<Vec<&'a Adapter>, Trouble> {
    if name.is_empty() {
        return Ok(adapters.iter().collect());
    }
    let named = |adapter: &Adapter| adapter.name_of_counter == name;
    if let Some(exact) = adapters.iter().find(|adapter| named(adapter)) {
        return Ok(vec![exact]);
    }
    let close: Vec<&Adapter> =
        adapters.iter().filter(|one| one.name_of_counter.contains(name)).collect();
    match close.as_slice() {
        [] => Err(Trouble::Said(format!("no counter is named {name}"))),
        [one] => {
            writeln!(out, "{}", style::DETAIL.paint(&format!(
                    "no counter is named {name}, so this is {}", one.name_of_counter)))?;
            Ok(vec![one])
        }
        several => {
            let names: Vec<&str> =
                several.iter().map(|one| one.name_of_counter.as_str()).collect();
            Err(Trouble::Said(format!(
                "no counter is named {name}, and more than one contains it:\n  {}",
                names.join("\n  ")
            )))
        }
    }
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
        for fault in faults.iter() {
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
            assert!(settings_of(&args).unwrap().help.is_some(), "{args:?}");
        }
    }

    #[test]
    fn a_usage_line_is_cut_into_its_flags_and_what_lies_between_them() {
        let cut = cut_the_arguments("[<case>] [--counter <name>] [--bin <path>]");
        assert_eq!(cut[0], (style::PLAIN, "[<case>] [".to_string()));
        assert_eq!(cut[1], (style::FLAG, "--counter".to_string()));
        assert_eq!(cut[2], (style::PLAIN, " <name>] [".to_string()));
        assert_eq!(cut[3], (style::FLAG, "--bin".to_string()));

        // Nothing may be added or lost, however odd the line.
        for line in ["", "[]", "--", "-h", "a -- b", "[--last]"] {
            let kept: String =
                cut_the_arguments(line).iter().map(|(_, piece)| piece.as_str()).collect();
            assert_eq!(kept, line, "{line}");
        }
    }

    // Asking a command for its help is asking about that command, so it answers with itself. The
    // whole of it, the note about colour included, is what a bare --help is for.
    #[test]
    fn help_after_a_command_is_that_commands_own_and_a_bare_one_is_the_whole_of_it() {
        let one = settings_of(&["fetch", "--help"]).unwrap().help.unwrap();
        assert!(one.contains("linejudge fetch "), "{one}");
        assert!(!one.contains("linejudge check "), "{one}");
        assert!(!one.contains("CLICOLOR_FORCE"), "{one}");

        let whole = settings_of(&["--help"]).unwrap().help.unwrap();
        for named in ["check", "explain", "fetch", "render"] {
            assert!(whole.contains(&format!("linejudge {named} ")), "{named} is missing");
        }
        assert!(whole.contains("CLICOLOR_FORCE"), "the note about colour is missing");
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
