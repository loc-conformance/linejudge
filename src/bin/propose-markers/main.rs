#![forbid(unsafe_code)]

// This is a supporting tool that helps us generate the truth.txt of every case of the corpus.
// It paints marker lines, it does not decide them: a person says where the spans are and this
// fills in the columns. Nothing here judges its own output, the corpus reader does.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = r#"propose-markers <spec>

    Run from the root of this repository, it writes the truth.txt of every case the spec names.
    A truth.txt carries one marker line under a copy of each source line, saying of every byte
    whether it is inside a string, inside a comment, part of a tag that bounds another language,
    or none of those. You say where the spans and the tags are; this only fills in the columns,
    so read what it wrote before believing it. 'cargo test' then refuses a truth that does not
    match its input.

    Case 0550 is a worked example. Its first line is

        /* block */ // trailing

    which holds two comments, so its spec is

        == 0550
        1 C /*| block |*/
        1 C //| trailing

    In case 0550, on line 1, there is a comment that the symbol /* opens and the symbol */
    closes, and another that // opens and nothing closes, so it runs to the end of the line.
    The first bar ends the opening symbol, the second begins the closing one; with no second
    bar the span runs to the end of the line, and a closing bar at the very end, like \|"|,
    bounds the span without a closing symbol. What comes out is

        /* block */ // trailing
        CCcccccccCC CCccccccccc

    where a capital marks a byte of a symbol, a small letter the inside of the span, a dot a
    byte that belongs to no span, and a space stays a space.

    A span that crosses lines is one row naming its whole range, with ' ... ' standing for the
    lines between, which need nothing said about them:

        3-5 C /*| ... |*/

    A tag that opens a stretch of another language is a row of kind >, the language in
    parentheses on the first row of the tag, and the tag that closes it a row of kind <; the
    same parentheses on an S or C row label the span itself as the region, a doc comment read
    as Markdown being the one in the corpus:

        2 > (JavaScript) <SCRIPT>
        5 < </SCRIPT>
        1 C (Markdown optional) ///| A documented function.

    Lines are numbered from 1 and a line no row names comes out as all dots, which is what a
    line of plain code is. Rows touching one line are written in the order they sit on it, and
    each is searched for after the previous one ends. A text that is still ambiguous is refused,
    and the refusal is answered by appending @2 for the second occurrence.
"#;

const OPTIONAL_WORD: &str = "optional";

enum Item {
    Span {
        from: usize,
        to: usize,
        kind: char,
        label: Option<String>,
        opener: String,
        interior: String,
        closer: Option<String>,
        occurrence: Option<usize>,
    },
    Tag {
        line: usize,
        kind: char,
        label: Option<String>,
        text: String,
        occurrence: Option<usize>,
    },
}

fn main() -> ExitCode {
    let Some(spec_path) = env::args().nth(1).filter(|arg| !arg.starts_with('-')) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    match run(&spec_path) {
        Ok(written) => {
            println!("{written} truths written");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run(spec_path: &str) -> Result<usize, String> {
    let spec = fs::read_to_string(spec_path).map_err(|e| format!("{spec_path}: {e}"))?;
    let cases = parse_spec(&spec)?;
    for (id, items) in &cases {
        write_truth(id, items)?;
    }
    Ok(cases.len())
}

fn parse_spec(spec: &str) -> Result<Vec<(String, Vec<Item>)>, String> {
    let mut cases: Vec<(String, Vec<Item>)> = Vec::new();
    let mut id = String::new();
    let mut items: Vec<Item> = Vec::new();

    for row in spec.lines().chain(std::iter::once("==")) {
        let row = row.trim_end();
        if row.starts_with("//") || row.is_empty() {
            continue;
        }
        if let Some(next) = row.strip_prefix("==") {
            if !id.is_empty() {
                cases.push((std::mem::take(&mut id), std::mem::take(&mut items)));
            }
            id = next.trim().to_string();
            continue;
        }
        items.push(parse_row(row).map_err(|e| format!("{id}: {e}"))?);
    }
    Ok(cases)
}

fn parse_row(row: &str) -> Result<Item, String> {
    let (range, rest) = row.split_once(' ').ok_or_else(|| format!("[{row}] names nothing"))?;
    let (from, to) = match range.split_once('-') {
        Some((from, to)) => (parse_line(from)?, parse_line(to)?),
        None => (parse_line(range)?, parse_line(range)?),
    };
    let (kind, rest) = rest.split_once(' ').ok_or_else(|| format!("[{row}] has no text"))?;
    let kind = match kind {
        "S" | "C" | ">" | "<" => kind.chars().next().unwrap_or('S'),
        _ => return Err(format!("[{kind}] is not S, C, > or <")),
    };
    let (label, text) = split_label(rest.trim_start())?;
    let (text, occurrence) = split_occurrence(text);

    if kind == '>' || kind == '<' {
        if from != to {
            return Err(format!("[{row}] gives a tag a range, and a tag row is one line"));
        }
        return Ok(Item::Tag { line: from, kind, label, text: text.to_string(), occurrence });
    }

    let mut parts = text.split('|');
    let opener = parts.next().unwrap_or_default().to_string();
    let Some(interior) = parts.next() else {
        return Err(format!("[{text}] has no bar after its opening symbol"));
    };
    let closer = parts.next().map(str::to_string);
    if parts.next().is_some() {
        return Err(format!("[{text}] holds more bars than a span can mean"));
    }
    if from != to && interior.trim() != "..." {
        return Err(format!("[{text}] crosses lines, so its middle is written ' ... '"));
    }
    Ok(Item::Span {
        from,
        to,
        kind,
        label,
        opener,
        interior: interior.to_string(),
        closer,
        occurrence,
    })
}

fn parse_line(text: &str) -> Result<usize, String> {
    text.parse().map_err(|_| format!("[{text}] is no line number"))
}

fn split_label(text: &str) -> Result<(Option<String>, &str), String> {
    let Some(inside) = text.strip_prefix('(') else { return Ok((None, text)) };
    let Some(end) = inside.find(')') else { return Ok((None, text)) };
    let named = &inside[..end];
    if named.contains('|') {
        return Ok((None, text));
    }
    let label = match named.strip_suffix(OPTIONAL_WORD) {
        Some(language) => format!("{} ({OPTIONAL_WORD})", language.trim()),
        None => named.trim().to_string(),
    };
    Ok((Some(label), inside[end + 1..].trim_start()))
}

fn split_occurrence(text: &str) -> (&str, Option<usize>) {
    let Some(at) = text.rfind('@') else { return (text, None) };
    match text[at + 1..].parse::<usize>() {
        Ok(number) if number > 0 => (&text[..at], Some(number)),
        _ => (text, None),
    }
}

fn write_truth(id: &str, items: &[Item]) -> Result<(), String> {
    let dir = find_case(id)?;
    let input = fs::read_dir(&dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .flatten()
        .map(|entry| entry.path())
        .find(|path| path.file_name().is_some_and(|n| n.to_string_lossy().starts_with("input.")))
        .ok_or_else(|| format!("{id} has no input"))?;
    let text = fs::read_to_string(&input).map_err(|e| format!("{}: {e}", input.display()))?;
    let truth = build_truth(id, items, &text.lines().collect::<Vec<&str>>())?;
    fs::write(dir.join("truth.txt"), truth).map_err(|e| format!("{id}: {e}"))
}

fn build_truth(id: &str, items: &[Item], sources: &[&str]) -> Result<String, String> {
    let mut markers: Vec<Vec<u8>> = sources
        .iter()
        .map(|line| {
            line.bytes().map(|byte| if byte.is_ascii_whitespace() { byte } else { b'.' }).collect()
        })
        .collect();
    let mut lone: BTreeMap<usize, u8> = BTreeMap::new();
    let mut labels: BTreeMap<usize, String> = BTreeMap::new();
    let mut cursors: BTreeMap<usize, usize> = BTreeMap::new();

    for item in items {
        paint(id, item, sources, &mut markers, &mut lone, &mut labels, &mut cursors)?;
    }

    let mut out = String::new();
    for (index, source) in sources.iter().enumerate() {
        out.push_str(source);
        out.push('\n');
        let mut marker = if source.is_empty() {
            lone.get(&index).map(|byte| (*byte as char).to_string()).unwrap_or_default()
        } else {
            String::from_utf8(markers[index].clone()).map_err(|e| format!("{id}: {e}"))?
        };
        if let Some(label) = labels.get(&index) {
            marker.push(' ');
            marker.push_str(label);
        }
        if !marker.is_empty() {
            out.push_str(&marker);
            out.push('\n');
        }
    }
    Ok(out)
}

fn paint(
    id: &str,
    item: &Item,
    sources: &[&str],
    markers: &mut [Vec<u8>],
    lone: &mut BTreeMap<usize, u8>,
    labels: &mut BTreeMap<usize, String>,
    cursors: &mut BTreeMap<usize, usize>,
) -> Result<(), String> {
    match item {
        Item::Tag { line, kind, label, text, occurrence } => {
            let index = line - 1;
            let at = find_text(id, sources, index, text, *occurrence, cursors)?;
            for offset in 0..text.len() {
                markers[index][at + offset] = *kind as u8;
            }
            note_label(id, labels, index, label)?;
            Ok(())
        }
        Item::Span { from, to, kind, label, opener, interior, closer, occurrence } => {
            let upper = kind.to_ascii_uppercase() as u8;
            let lower = kind.to_ascii_lowercase() as u8;
            let first = from - 1;
            let last = to - 1;
            if from == to {
                let text = format!("{opener}{interior}{}", closer.as_deref().unwrap_or_default());
                let at = find_text(id, sources, first, &text, *occurrence, cursors)?;
                paint_run(&mut markers[first][at..at + text.len()], lower);
                paint_run(&mut markers[first][at..at + opener.len()], upper);
                let closing = closer.as_deref().unwrap_or_default().len();
                paint_run(&mut markers[first][at + text.len() - closing..at + text.len()], upper);
                if closer.is_none() {
                    paint_run(&mut markers[first][at + opener.len()..], lower);
                }
                note_label(id, labels, first, label)?;
                return Ok(());
            }
            let at = find_text(id, sources, first, opener, None, cursors)?;
            paint_run(&mut markers[first][at..], lower);
            paint_run(&mut markers[first][at..at + opener.len()], upper);
            cursors.insert(first, sources[first].len());
            for index in first + 1..last {
                if sources[index].is_empty() {
                    lone.insert(index, lower);
                } else {
                    paint_run(&mut markers[index], lower);
                }
            }
            match closer {
                // A trailing @N on a crossing span picks the closer's occurrence, since the
                // opener was already anchored by where the span begins.
                Some(closing) => {
                    let at = find_text(id, sources, last, closing, *occurrence, cursors)?;
                    paint_run(&mut markers[last][..at], lower);
                    paint_run(&mut markers[last][at..at + closing.len()], upper);
                }
                None => paint_run(&mut markers[last], lower),
            }
            note_label(id, labels, first, label)?;
            Ok(())
        }
    }
}

fn paint_run(run: &mut [u8], mark: u8) {
    for byte in run {
        *byte = mark;
    }
}

fn note_label(
    id: &str,
    labels: &mut BTreeMap<usize, String>,
    index: usize,
    label: &Option<String>,
) -> Result<(), String> {
    let Some(label) = label else { return Ok(()) };
    if labels.insert(index, label.clone()).is_some() {
        return Err(format!("{id} line {}: two labels on one line", index + 1));
    }
    Ok(())
}

/// Refuses an ambiguous text instead of guessing: the tool cannot know which occurrence is the
/// span, that would take knowing the language, but it can count them and demand an @2.
fn find_text(
    id: &str,
    sources: &[&str],
    index: usize,
    text: &str,
    occurrence: Option<usize>,
    cursors: &mut BTreeMap<usize, usize>,
) -> Result<usize, String> {
    let source = sources
        .get(index)
        .ok_or_else(|| format!("{id} line {}: the input has no such line", index + 1))?;
    if text.is_empty() {
        return Err(format!("{id} line {}: an empty text", index + 1));
    }
    let cursor = *cursors.get(&index).unwrap_or(&0);
    let hits: Vec<usize> =
        source[cursor..].match_indices(text).map(|(at, _)| cursor + at).collect();
    let chosen = match (hits.len(), occurrence) {
        (0, _) => {
            return Err(format!("{id} line {}: [{text}] is not in it past column {cursor}", index + 1));
        }
        (1, None) => hits[0],
        (found, None) => {
            return Err(format!(
                "{id} line {}: [{text}] is there {found} times, say which with @2",
                index + 1
            ));
        }
        (found, Some(wanted)) if wanted <= found => hits[wanted - 1],
        (found, Some(wanted)) => {
            return Err(format!(
                "{id} line {}: [{text}] is asked for as @{wanted} and is there {found} times",
                index + 1
            ));
        }
    };
    cursors.insert(index, chosen + text.len());
    Ok(chosen)
}

fn find_case(id: &str) -> Result<PathBuf, String> {
    let entries = fs::read_dir("cases").map_err(|e| format!("cases: {e}"))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(&format!("{id}-")) {
            return Ok(entry.path());
        }
    }
    Err(format!("no case starts with {id}-"))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use linejudge::truth::Truth;

    use super::*;

    // One file holding every shape at once, so that what the single-shape tests below cannot see,
    // the interactions between them, is covered by something a person can read whole. It is not
    // a case and the corpus never touches it: a hand correction to a case is free to happen
    // without breaking a test of this tool.
    #[test]
    fn the_golden_fixture_is_painted_byte_for_byte_and_the_reader_accepts_what_came_out() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/proposer");
        let read = |name: &str| {
            fs::read_to_string(dir.join(name))
                .unwrap_or_else(|e| panic!("{name}: {e}"))
                .replace("\r\n", "\n")
        };
        let input = read("input.html");
        let spec = read("spec.txt");
        let golden = read("truth.txt");

        let cases = parse_spec(&spec).unwrap();
        assert_eq!(cases.len(), 1, "the fixture spec names one case");
        let painted =
            build_truth("the golden", &cases[0].1, &input.lines().collect::<Vec<&str>>()).unwrap();
        assert_eq!(painted, golden);

        let truth = Truth::read(&golden, &input).unwrap_or_else(|f| panic!("{f:?}"));
        let language = |at: usize| {
            truth.lines[at].region.as_ref().map(|claim| claim.language.as_str())
        };
        assert_eq!(language(3), None, "the tag line stays with the page");
        assert_eq!(language(4), Some("TypeScript"));
        assert_eq!(language(5), Some("Markdown"), "the labeled doc block nests inside the script");
        assert_eq!(language(10), Some("TypeScript"));
        assert_eq!(language(15), None, "and so does the closing tag line");
        assert_eq!(language(17), Some("SCSS"));
        assert_eq!(language(19), None);
    }

    #[test]
    fn two_spans_on_one_line_are_painted_in_the_order_they_sit_on_it() {
        let truth = propose(&["/* block */ // trailing"], &["1 C /*| block |*/", "1 C //| trailing"]);
        assert_eq!(truth, "/* block */ // trailing\nCCcccccccCC CCccccccccc\n");
    }

    #[test]
    fn a_span_with_no_closing_bar_runs_to_the_end_of_the_line() {
        let truth = propose(&["x = 1; // a note"], &["1 C //| a note"]);
        assert_eq!(truth, "x = 1; // a note\n. . .. CCccccccc\n");
    }

    #[test]
    fn a_closing_bar_at_the_very_end_bounds_a_span_that_has_no_closing_symbol() {
        let truth = propose(&["(def q \\\")"], &["1 S \\|\"|"]);
        assert_eq!(truth, "(def q \\\")\n.... . Ss.\n");
    }

    #[test]
    fn a_crossing_span_owns_the_lines_between_its_symbols_whole() {
        let source = ["value = \"\"\"", "first", "", "third", "\"\"\"", "print(value)"];
        let truth = propose(&source, &["1-5 S \"\"\"| ... |\"\"\""]);
        assert_eq!(
            truth,
            "value = \"\"\"\n..... . SSS\nfirst\nsssss\n\ns\nthird\nsssss\n\"\"\"\nSSS\n\
             print(value)\n............\n"
        );
    }

    #[test]
    fn a_tag_pair_is_marked_and_the_language_is_written_after_the_columns() {
        let source = ["<script>", "var x = 1;", "</script>"];
        let truth = propose(&source, &["1 > (JavaScript) <script>", "3 < </script>"]);
        assert_eq!(
            truth,
            "<script>\n>>>>>>>> JavaScript\nvar x = 1;\n... . . ..\n</script>\n<<<<<<<<<\n"
        );
    }

    #[test]
    fn a_tag_that_spans_lines_keeps_its_attribute_string_and_takes_one_label() {
        let source = ["<script", "  type=\"text/javascript\">", "</script>"];
        let spec = ["1 > (JavaScript) <script", "2 S \"|text/javascript|\"", "2 > >", "3 < </script>"];
        let truth = propose(&source, &spec);
        assert_eq!(
            truth,
            "<script\n>>>>>>> JavaScript\n  type=\"text/javascript\">\n\
             \x20 .....SsssssssssssssssS>\n</script>\n<<<<<<<<<\n"
        );
    }

    #[test]
    fn a_label_on_a_span_row_names_the_region_that_span_carries() {
        let truth = propose(&["/// a doc line"], &["1 C (Markdown optional) ///| a doc line"]);
        assert_eq!(truth, "/// a doc line\nCCCccccccccccc Markdown (optional)\n");
    }

    // 6400's second line, where the closer is there twice: once ending the comment above and once
    // inside the '*/*' whose second half reopens it. The tool cannot know which, and says so.
    #[test]
    fn a_text_that_is_there_twice_is_refused_until_an_occurrence_is_named() {
        let source = ["int w = 8; /* tail", "*/ int z = 9; */* int y = 10;"];
        let refused = refuse(&source, &["1-2 C /*| ... |*/"]);
        assert!(refused.contains("say which with @2"), "{refused}");
        let truth = propose(&source, &["1-2 C /*| ... |*/@1"]);
        assert_eq!(
            truth,
            "int w = 8; /* tail\n... . . .. CCccccc\n*/ int z = 9; */* int y = 10;\n\
             CC ... . . .. ... ... . . ...\n"
        );
    }

    #[test]
    fn a_row_the_grammar_does_not_allow_is_refused_with_what_is_wrong() {
        let refused = refuse(&["x = 1"], &["1 X //| nope"]);
        assert!(refused.contains("is not S, C, > or <"), "{refused}");
        let refused = refuse(&["x = 1"], &["1 C no bars here"]);
        assert!(refused.contains("no bar after its opening symbol"), "{refused}");
        let refused = refuse(&["x = 1", "y = 2"], &["1-2 C /*| middle |*/"]);
        assert!(refused.contains("' ... '"), "{refused}");
        let refused = refuse(&["<script>"], &["1-2 > (CSS) <script>"]);
        assert!(refused.contains("a tag row is one line"), "{refused}");
    }

    fn propose(sources: &[&str], spec: &[&str]) -> String {
        build_truth("a test", &parse(spec), sources).unwrap()
    }

    fn refuse(sources: &[&str], spec: &[&str]) -> String {
        let items = match spec.iter().map(|row| parse_row(row)).collect::<Result<Vec<_>, _>>() {
            Ok(items) => items,
            Err(refused) => return refused,
        };
        build_truth("a test", &items, sources).unwrap_err()
    }

    fn parse(spec: &[&str]) -> Vec<Item> {
        spec.iter().map(|row| parse_row(row).unwrap()).collect()
    }
}
