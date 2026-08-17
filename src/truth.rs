const ALPHABET: &str = "SsZCcU.>< \t";
pub const COMMENT_MARKS: Marks = Marks { name: "comment", opener: 'C', interior: 'c', closer: 'U' };
const KINDS: [Marks; 2] = [STRING_MARKS, COMMENT_MARKS];
const OPTIONAL_WORD: &str = "optional";
pub const RESIDUE: char = '.';
pub const STRING_MARKS: Marks = Marks { name: "string", opener: 'S', interior: 's', closer: 'Z' };
pub const TAG_CLOSES: char = '<';
pub const TAG_OPENS: char = '>';

/// The hand-verified spans of one case, read from its `truth.txt`: under a copy of every source
/// line, one marker character per character, `S` `s` `Z` for a string's opening symbol, its bytes
/// and its closing symbol, `C` `c` `U` for a comment's, `.` for a byte outside every span,
/// whitespace outside spans kept as it stands, and a blank line that a span is open across carrying
/// a lone `s` or `c`. A tag that opens a stretch of another language is marked `>` and the tag that
/// closes it `<`, with the language named after the markers of every line the opening tag covers; a
/// region with no tag is the same name on the line where its span opens. Which lines belong to a
/// region is computed, never declared.
#[derive(Debug)]
pub struct Truth {
    pub lines: Vec<TruthLine>,
}

impl Truth {
    /// The input is the one file the truth describes, and every refusal names what went stale:
    /// a copy that no longer matches the input is the loud failure the copies exist for.
    pub fn read(marked: &str, input: &str) -> Result<Truth, Vec<String>> {
        let mut faults = Vec::new();
        let mut lines = Vec::new();
        let mut rows = marked.lines().peekable();

        for source in input.lines() {
            let Some(copy) = rows.next() else {
                faults.push("the truth ends before its input does".to_string());
                return Err(faults);
            };
            if copy != source {
                faults.push(format!("a copy differs from the input: [{source}] against [{copy}]"));
                return Err(faults);
            }
            if !source.is_ascii() {
                faults.push(format!(
                    "[{source}] holds a character above ASCII, and a case input has to be ASCII: \
                     markers are written one to a byte and read one to a character, and the two \
                     agree only there"
                ));
                return Err(faults);
            }
            if source.is_empty() {
                let marker = match rows.peek() {
                    Some(&"s") | Some(&"c") => rows.next().unwrap_or_default().to_string(),
                    _ => String::new(),
                };
                lines.push(RawLine { source: source.to_string(), marker, label: None });
                continue;
            }
            let Some(row) = rows.next() else {
                faults.push(format!("no marker line under [{source}]"));
                return Err(faults);
            };
            match split_marker(row, source) {
                Ok((marker, label)) => {
                    lines.push(RawLine { source: source.to_string(), marker, label })
                }
                Err(message) => faults.push(message),
            }
        }
        if let Some(extra) = rows.next() {
            faults.push(format!("the truth holds [{extra}] past the end of its input"));
        }
        // A marker that did not parse leaves no line behind, so the walk below would be reading
        // one file's markers against another file's lines.
        if faults.is_empty() {
            check_span_structure(&lines, &mut faults);
        }
        if !faults.is_empty() {
            return Err(faults);
        }

        let regions = assign_regions(&lines)?;
        Ok(Truth {
            lines: lines
                .into_iter()
                .zip(regions)
                .map(|(raw, regions)| TruthLine {
                    source: raw.source,
                    marker: raw.marker,
                    regions,
                })
                .collect(),
        })
    }
}

#[derive(Debug)]
pub struct TruthLine {
    /// Kept beside the marker because the rules need to know what a character is, a letter or a
    /// space, and the marker only says which string or comment it belongs to.
    pub source: String,
    pub marker: String,
    /// Every region this line is inside, the outermost first. A counter that does not count an
    /// optional region on its own still needs the region around it, so the whole stack is kept.
    pub regions: Vec<RegionClaim>,
}

impl TruthLine {
    /// Which region this line counts towards: the innermost one this counter counts on its own,
    /// where `counted_readings` names the readings it does count. A counter that leaves one out
    /// gives the line to the region around it, and `None` means the file itself.
    pub fn find_region(&self, counted_readings: &[&str]) -> Option<&RegionClaim> {
        self.regions.iter().rev().find(|claim| match &claim.reading {
            None => true,
            Some(reading) => counted_readings.contains(&reading.as_str()),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionClaim {
    pub language: String,
    /// The reading under which this stretch is a language of its own, where counting it apart and
    /// leaving it to the code around it are both fair, and `None` where it is not in question. It
    /// names the question a counter answers, `rust-doc-comment` rather than `Markdown`, because two
    /// counters can answer differently about a doc comment and the same about a Vue template.
    pub reading: Option<String>,
}

/// The three marker characters of one kind of span, so that whoever writes a marker line and
/// whoever reads it take the alphabet from the same place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Marks {
    pub name: &'static str,
    pub opener: char,
    pub interior: char,
    pub closer: char,
}

impl Marks {
    pub fn owns(&self, mark: char) -> bool {
        self.find_role(mark).is_some()
    }

    fn find_role(&self, mark: char) -> Option<Role> {
        match mark {
            _ if mark == self.opener => Some(Role::Opens),
            _ if mark == self.interior => Some(Role::Inside),
            _ if mark == self.closer => Some(Role::Closes),
            _ => None,
        }
    }
}

struct RawLine {
    source: String,
    marker: String,
    label: Option<RegionClaim>,
}

enum Role {
    Opens,
    Inside,
    Closes,
}

fn split_marker(row: &str, source: &str) -> Result<(String, Option<RegionClaim>), String> {
    let width = source.chars().count();
    let marker: String = row.chars().take(width).collect();
    if marker.chars().count() < width {
        return Err(format!("the marker under [{source}] is not as long as it"));
    }
    if let Some(wrong) = marker.chars().find(|ch| !ALPHABET.contains(*ch)) {
        return Err(format!("'{wrong}' under [{source}] is not on the alphabet"));
    }
    let excess: String = row.chars().skip(width).collect();
    if excess.is_empty() {
        return Ok((marker, None));
    }
    if !excess.starts_with(' ') {
        return Err(format!("the marker under [{source}] runs past the end of the line"));
    }
    let opens_a_span = marker.chars().any(|ch| find_marks_of_opener(ch).is_some());
    if !marker.contains(TAG_OPENS) && !opens_a_span {
        return Err(format!("[{}] labels a line that opens no tag and no span", excess.trim()));
    }
    let named = excess.trim();
    let (language, reading) = split_reading_off(named)?;
    if language.is_empty() || !language.chars().next().is_some_and(char::is_alphanumeric) {
        return Err(format!("[{named}] does not name a language"));
    }
    Ok((marker, Some(RegionClaim { language: language.to_string(), reading })))
}

// A span ends at its closing symbol where it has one, and where it has none, a line comment or a
// character literal written `\"`, it ends where its own bytes stop: at the end of its line, or at
// the first byte that is not its interior. So a mark that is not this span's says only that the
// span was over, which leaves one thing a marker cannot explain and this refuses, the bytes or
// the closing symbol of a span where no such span is open.
fn check_span_structure(lines: &[RawLine], faults: &mut Vec<String>) {
    let mut open: Option<Marks> = None;
    for line in lines {
        if let Some(marks) = open
            && !line.marker.starts_with([marks.interior, marks.closer])
        {
            open = None;
        }
        let source = &line.source;
        let mut marker = line.marker.chars().peekable();
        while let Some(mark) = marker.next() {
            while marker.peek() == Some(&mark) {
                marker.next();
            }
            let found =
                KINDS.into_iter().find_map(|marks| marks.find_role(mark).map(|role| (marks, role)));
            let Some((marks, role)) = found else {
                open = None;
                continue;
            };
            match (role, open) {
                (Role::Opens, _) => open = Some(marks),
                (Role::Closes, Some(already)) if already == marks => open = None,
                (Role::Closes, _) => {
                    faults.push(format!("[{source}] closes a {} where none is open", marks.name))
                }
                (Role::Inside, Some(already)) if already == marks => {}
                (Role::Inside, _) => faults.push(format!(
                    "[{source}] holds the bytes of a {} where none is open",
                    marks.name
                )),
            }
        }
    }
}

/// A label is `HTML` where the region is not in question and `HTML (optional vue-template)` where
/// it is. The reading is demanded rather than defaulted: a bare `(optional)` would leave every
/// dialect answering one question for two different ones.
fn split_reading_off(named: &str) -> Result<(&str, Option<String>), String> {
    let Some((language, bracketed)) = named.split_once('(') else { return Ok((named, None)) };
    let Some(inside) = bracketed.strip_suffix(')') else {
        return Err(format!("[{named}] leaves its bracket unclosed"));
    };
    let Some(reading) = inside.trim().strip_prefix(OPTIONAL_WORD) else {
        return Err(format!("[{named}] says something other than {OPTIONAL_WORD} in its bracket"));
    };
    let reading = reading.trim();
    if reading.is_empty() {
        return Err(format!(
            "[{named}] does not name the reading it is optional under, such as \
             (optional vue-template)"
        ));
    }
    Ok((language.trim(), Some(reading.to_string())))
}

// Regions nest, a php page holding markup that holds a script being the everyday file, and every
// line belongs to the innermost one: the tags are a stack, and a tag's own lines belong to the
// region that encloses the tag, which for a top-level tag is the file itself.
//
// Every line of a tag that opens names its language, a tag written over two lines naming it twice,
// so nothing here is worked out from where a line sits: the same language again is the tag above
// carrying on, a different one is a region opening inside it, and no language at all is refused.
// Two regions of one language cannot nest, since a boundary from a language to itself is not one.
fn assign_regions(lines: &[RawLine]) -> Result<Vec<Vec<RegionClaim>>, Vec<String>> {
    let mut faults = Vec::new();
    let mut regions: Vec<Vec<RegionClaim>> = lines.iter().map(|_| Vec::new()).collect();

    let mut stack: Vec<RegionClaim> = Vec::new();
    let mut named_above: Option<&RegionClaim> = None;
    for (index, line) in lines.iter().enumerate() {
        let opens = line.marker.contains(TAG_OPENS);
        let closes = line.marker.contains(TAG_CLOSES);
        let mut named_here = None;
        if opens && closes {
            faults.push("a line both opens and closes a tag, which is not yet a shape".to_string());
        } else if opens {
            match &line.label {
                Some(claim) => {
                    match named_above {
                        Some(above) if above == claim => {}
                        Some(above) if above.language == claim.language => faults.push(format!(
                            "one opening tag calls {} optional on one of its lines and not on the \
                             other",
                            claim.language
                        )),
                        _ => stack.push(claim.clone()),
                    }
                    named_here = Some(claim);
                }
                None => faults.push("an opening tag names no language".to_string()),
            }
            regions[index] = stack[..stack.len().saturating_sub(1)].to_vec();
        } else if closes {
            if stack.pop().is_none() {
                faults.push("a closing tag closes nothing".to_string());
            }
            regions[index] = stack.clone();
        } else {
            regions[index] = stack.clone();
        }
        named_above = named_here;
    }
    if !stack.is_empty() {
        faults.push("an opening tag is never closed".to_string());
    }

    claim_labeled_spans(lines, &mut regions, &mut faults);
    if faults.is_empty() { Ok(regions) } else { Err(faults) }
}

// A label on a span-opening line claims the consecutive lines wholly covered by that kind that
// either continue the span opened above it, its closing symbol included, or open a new one with
// the same bytes, so a `///` block ends where a plain `//` begins. Inside a tagged region the
// claim nests, the innermost language winning as everywhere; only two labeled spans running into
// each other have no meaning.
fn claim_labeled_spans(
    lines: &[RawLine],
    regions: &mut [Vec<RegionClaim>],
    faults: &mut Vec<String>,
) {
    let mut labeled: Vec<bool> = lines.iter().map(|_| false).collect();
    for (index, line) in lines.iter().enumerate() {
        let Some(claim) = &line.label else { continue };
        if line.marker.contains(TAG_OPENS) {
            continue;
        }
        let Some((marks, signature)) = read_last_opener(line) else {
            continue;
        };
        for (at, this) in lines.iter().enumerate().skip(index) {
            let belongs = at == index
                || (is_wholly_covered(this, marks)
                    && (begins_inside(this, marks) || read_opening_bytes(this, marks) == signature));
            if !belongs {
                break;
            }
            if labeled[at] {
                faults.push(format!("the {} label runs into another labeled span", claim.language));
                break;
            }
            regions[at].push(claim.clone());
            labeled[at] = true;
        }
    }
}

fn find_marks_of_opener(mark: char) -> Option<Marks> {
    KINDS.into_iter().find(|marks| marks.opener == mark)
}

fn is_wholly_covered(line: &RawLine, marks: Marks) -> bool {
    !line.marker.is_empty() && line.marker.chars().all(|ch| marks.find_role(ch).is_some())
}

fn begins_inside(line: &RawLine, marks: Marks) -> bool {
    line.marker.starts_with([marks.interior, marks.closer])
}

/// The kind and the opening bytes of the last span the line opens. Only the last one can still be
/// open on the next line, so a line holding a string and then a doc comment carries its region with
/// the comment, and a line holding two comments is signed by the second of them.
fn read_last_opener(line: &RawLine) -> Option<(Marks, String)> {
    let mut last: Option<(Marks, String)> = None;
    let mut running = false;
    for (byte, mark) in line.source.chars().zip(line.marker.chars()) {
        match find_marks_of_opener(mark) {
            Some(marks) => {
                match &mut last {
                    Some((already, bytes)) if running && *already == marks => bytes.push(byte),
                    _ => last = Some((marks, byte.to_string())),
                }
                running = true;
            }
            None => running = false,
        }
    }
    last
}

/// The source bytes under the line's first run of opening marks of this kind: `///` under `CCC`,
/// `"""` under `SSS`. What tells a doc comment from the plain comment beside it.
fn read_opening_bytes(line: &RawLine, marks: Marks) -> String {
    line.source
        .chars()
        .zip(line.marker.chars())
        .skip_while(|(_, mark)| *mark != marks.opener)
        .take_while(|(_, mark)| *mark == marks.opener)
        .map(|(byte, _)| byte)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_valid_truth_pairs_every_line_and_derives_its_tagged_region() {
        let input = "<script>\n// inside\n</script>\n<p>x</p>\n";
        let marked = "<script>\n>>>>>>>> JavaScript\n// inside\nCCccccccc\n</script>\n<<<<<<<<<\n\
                      <p>x</p>\n........\n";
        let truth = Truth::read(marked, input).unwrap();
        assert_eq!(truth.lines.len(), 4);
        assert_eq!(truth.lines[1].source, "// inside");
        let claim = &truth.lines[1].regions[0];
        assert_eq!(claim.language, "JavaScript");
        assert_eq!(claim.reading, None, "a tagged region is not in question");
        assert!(truth.lines[0].regions.is_empty(), "the tag line belongs to the parent");
        assert!(truth.lines[2].regions.is_empty());
        assert!(truth.lines[3].regions.is_empty());
    }

    #[test]
    fn a_pair_with_nothing_between_derives_a_region_that_owns_no_line() {
        let input = "<script>\n</script>\n<p>after</p>\n";
        let marked = "<script>\n>>>>>>>> JavaScript\n</script>\n<<<<<<<<<\n<p>after</p>\n............\n";
        let truth = Truth::read(marked, input).unwrap();
        assert!(truth.lines.iter().all(|line| line.regions.is_empty()));
    }

    #[test]
    fn a_labeled_doc_comment_covers_its_own_lines_and_stops_at_a_plain_one() {
        let input = "/// a\n///\n// plain\nfn x() {}\n";
        let marked = "/// a\nCCCcc Markdown (optional rust-doc-comment)\n///\nCCC\n\
                      // plain\nCCcccccc\nfn x() {}\n.. . ..\n";
        let refused = Truth::read(marked, input);
        // The last marker above is deliberately short, so first prove the length gate still runs.
        assert!(refused.is_err());
        let marked = "/// a\nCCCcc Markdown (optional rust-doc-comment)\n///\nCCC\n\
                      // plain\nCCcccccc\nfn x() {}\n.. ... ..\n";
        let truth = Truth::read(marked, input).unwrap();
        let markdown = |at: usize| truth.lines[at].regions.last().map(|c| c.language.as_str());
        assert_eq!(markdown(0), Some("Markdown"));
        assert_eq!(markdown(1), Some("Markdown"));
        assert_eq!(markdown(2), None, "a plain // is not part of the /// block");
        assert_eq!(truth.lines[0].regions[0].reading.as_deref(), Some("rust-doc-comment"));
    }

    #[test]
    fn a_labeled_multiline_span_carries_its_interior_and_its_closing_line() {
        let input = "\"\"\"\nnotes\n\"\"\"\nx = 1\n";
        let marked = "\"\"\"\nSSS Markdown (optional rust-doc-comment)\nnotes\nsssss\n\
                      \"\"\"\nZZZ\nx = 1\n. . .\n";
        let truth = Truth::read(marked, input).unwrap();
        assert!(!truth.lines[1].regions.is_empty());
        assert!(!truth.lines[2].regions.is_empty(), "the closing quotes match the opening bytes");
        assert!(truth.lines[3].regions.is_empty());
    }

    #[test]
    fn a_label_belongs_to_the_last_span_its_line_opens_and_not_to_an_earlier_one() {
        let input = "x = \"a\" /** doc\n * more\n */\n";
        let marked = "x = \"a\" /** doc\n. . SsZ CCCcccc Markdown (optional rust-doc-comment)\n\
                      \x20* more\nccccccc\n\x20*/\ncUU\n";
        let truth = Truth::read(marked, input).unwrap();
        let markdown = |at: usize| truth.lines[at].regions.last().map(|c| c.language.as_str());
        assert_eq!(markdown(0), Some("Markdown"));
        assert_eq!(markdown(1), Some("Markdown"), "the comment carries the region, not the string");
        assert_eq!(markdown(2), Some("Markdown"), "and its closing line with it");

        let input = "/* plain */ /// doc\n/// more\n";
        let marked = "/* plain */ /// doc\nCCcccccccUU CCCcccc Markdown\n/// more\nCCCccccc\n";
        let truth = Truth::read(marked, input).unwrap();
        let markdown = |at: usize| truth.lines[at].regions.last().map(|c| c.language.as_str());
        assert_eq!(markdown(0), Some("Markdown"));
        assert_eq!(markdown(1), Some("Markdown"), "the block is signed by /// and not by /*");
    }

    // A php page whose markup opens where the php stops and whose script opens on the very next
    // line, so both boundaries touch, and so do both closers.
    #[test]
    fn tags_that_touch_open_and_close_a_region_each() {
        let input = "?>\n<script>\nvar x = 1;\n</script>\n<?php\n";
        let marked = "?>\n>> HTML\n<script>\n>>>>>>>> JavaScript\nvar x = 1;\n... . . ..\n\
                      </script>\n<<<<<<<<<\n<?php\n<<<<<\n";
        let truth = Truth::read(marked, input).unwrap();
        let stack = |at: usize| {
            truth.lines[at].regions.iter().map(|c| c.language.as_str()).collect::<Vec<&str>>()
        };
        assert_eq!(stack(0), [] as [&str; 0]);
        assert_eq!(stack(1), ["HTML"], "the script tag belongs to the page around it");
        assert_eq!(stack(2), ["HTML", "JavaScript"]);
        assert_eq!(stack(3), ["HTML"], "the first of the two closers gives its line back");
        assert_eq!(stack(4), [] as [&str; 0]);
    }

    #[test]
    fn a_tag_written_over_two_lines_names_its_language_on_both_and_opens_one_region() {
        let input = "<script\n  type=\"text/javascript\">\nvar x = 1;\n</script>\n";
        let marked = "<script\n>>>>>>> JavaScript\n  type=\"text/javascript\">\n  \
                      .....SsssssssssssssssZ> JavaScript\nvar x = 1;\n... . . ..\n</script>\n\
                      <<<<<<<<<\n";
        let truth = Truth::read(marked, input).unwrap();
        assert!(truth.lines[0].regions.is_empty());
        assert!(truth.lines[1].regions.is_empty(), "both lines of the tag belong to the file");
        assert_eq!(truth.lines[2].regions.len(), 1, "and one region was opened, not two");
        assert_eq!(truth.lines[2].regions[0].language, "JavaScript");

        let unnamed = marked.replace("Z> JavaScript", "Z>");
        let refused = Truth::read(&unnamed, input).unwrap_err();
        assert!(refused[0].contains("names no language"), "{refused:?}");

        let disagreeing = marked.replace("Z> JavaScript", "Z> JavaScript (optional vue-template)");
        let refused = Truth::read(&disagreeing, input).unwrap_err();
        assert!(refused[0].contains("optional on one of its lines"), "{refused:?}");
    }

    #[test]
    fn an_opening_tag_without_a_language_and_an_orphan_closer_are_refused() {
        let unnamed = "<script>\n>>>>>>>>\nx\n.\n</script>\n<<<<<<<<<\n";
        let refused = Truth::read(unnamed, "<script>\nx\n</script>\n").unwrap_err();
        assert!(refused[0].contains("names no language"), "{refused:?}");
        let orphan = Truth::read("</script>\n<<<<<<<<<\n", "</script>\n").unwrap_err();
        assert!(orphan[0].contains("closes nothing"), "{orphan:?}");
        let unclosed = Truth::read("<script>\n>>>>>>>> CSS\n", "<script>\n").unwrap_err();
        assert!(unclosed[0].contains("never closed"), "{unclosed:?}");
    }

    // The everyday three-language file: a page's html holding a script, here in php's clothing.
    // Every line belongs to the innermost region, and a tag's own lines belong to the region
    // that encloses the tag, which for a top-level tag is the file itself.
    #[test]
    fn a_tag_pair_inside_a_tag_pair_nests_and_the_innermost_language_wins() {
        let input = "?>\n<div>\n<script>\nx\n</script>\n</div>\n<?php\n";
        let marked = "?>\n>> HTML\n<div>\n.....\n<script>\n>>>>>>>> JavaScript\nx\n.\n\
                      </script>\n<<<<<<<<<\n</div>\n......\n<?php\n<<<<<\n";
        let truth = Truth::read(marked, input).unwrap();
        let stack = |at: usize| {
            truth.lines[at].regions.iter().map(|c| c.language.as_str()).collect::<Vec<&str>>()
        };
        assert_eq!(stack(0), [] as [&str; 0], "the outer tag belongs to the file");
        assert_eq!(stack(1), ["HTML"]);
        assert_eq!(stack(2), ["HTML"], "the inner tag belongs to what encloses it");
        assert_eq!(stack(3), ["HTML", "JavaScript"]);
        assert_eq!(stack(4), ["HTML"]);
        assert_eq!(stack(5), ["HTML"]);
        assert_eq!(stack(6), [] as [&str; 0]);
    }

    #[test]
    fn a_labeled_span_inside_a_tagged_region_is_the_inner_region_of_the_two() {
        let input = "<script>\n/** doc */\n</script>\n";
        let marked = "<script>\n>>>>>>>> TypeScript\n/** doc */\n\
                      CCCcccccUU Markdown (optional rust-doc-comment)\n</script>\n<<<<<<<<<\n";
        let truth = Truth::read(marked, input).unwrap();
        let claims: Vec<&str> =
            truth.lines[1].regions.iter().map(|c| c.language.as_str()).collect();
        assert_eq!(claims, ["TypeScript", "Markdown"]);
        assert_eq!(truth.lines[1].regions[1].reading.as_deref(), Some("rust-doc-comment"));
    }

    #[test]
    fn a_dialect_that_declines_an_optional_region_is_given_the_one_around_it() {
        let input = "<script>\n/** doc */\n</script>\n";
        let marked = "<script>\n>>>>>>>> TypeScript\n/** doc */\n\
                      CCCcccccUU Markdown (optional rust-doc-comment)\n</script>\n<<<<<<<<<\n";
        let truth = Truth::read(marked, input).unwrap();
        let charged = |at: usize, counted: &[&str]| {
            truth.lines[at].find_region(counted).map(|claim| claim.language.as_str())
        };
        assert_eq!(charged(1, &["rust-doc-comment"]), Some("Markdown"));
        assert_eq!(charged(1, &[]), Some("TypeScript"));
        assert_eq!(charged(1, &["vue-template"]), Some("TypeScript"), "another reading is not it");
        assert_eq!(charged(0, &["rust-doc-comment"]), None, "the tag line is the file's anyway");
        assert_eq!(charged(0, &[]), None);
    }

    #[test]
    fn two_labeled_spans_running_into_each_other_are_refused() {
        let input = "/// one\n/// two\n";
        let marked = "/// one\nCCCcccc Markdown\n/// two\nCCCcccc Textile\n";
        let refused = Truth::read(marked, input).unwrap_err();
        assert!(refused[0].contains("runs into another labeled span"), "{refused:?}");
    }

    #[test]
    fn a_label_on_a_line_that_opens_nothing_is_refused() {
        let refused = Truth::read("int x = 1;\n... . . .. CSS\n", "int x = 1;\n").unwrap_err();
        assert!(refused[0].contains("opens no tag and no span"), "{refused:?}");
    }

    // The refusal is what a case author meets, and before it existed they met a marker reported as
    // running past the end of a line they had not touched.
    #[test]
    fn an_input_holding_a_character_above_ascii_is_refused_and_says_so() {
        let input = "let x = 1; // καλά\n";
        let refused = Truth::read("let x = 1; // καλά\n... . . .. CCcccccc\n", input).unwrap_err();
        assert!(refused[0].contains("above ASCII"), "{refused:?}");
    }

    #[test]
    fn a_copy_that_differs_from_the_input_is_the_loud_failure_the_copies_exist_for() {
        let refused = Truth::read("int y = 1;\n... . . ..\n", "int x = 1;\n").unwrap_err();
        assert!(refused[0].contains("differs from the input"), "{refused:?}");
    }

    #[test]
    fn a_short_marker_and_a_character_off_the_alphabet_are_both_named() {
        let refused = Truth::read("let x = 1;\n... . .\n", "let x = 1;\n").unwrap_err();
        assert!(refused[0].contains("not as long"), "{refused:?}");
        let refused = Truth::read("let x = 1;\n... . . .X\n", "let x = 1;\n").unwrap_err();
        assert!(refused[0].contains("'X'"), "{refused:?}");
    }

    #[test]
    fn a_truth_longer_or_shorter_than_its_input_is_refused() {
        let refused = Truth::read("a = 1\n. . .\nb = 2\n. . .\n", "a = 1\n").unwrap_err();
        assert!(refused[0].contains("past the end"), "{refused:?}");
        let refused = Truth::read("a = 1\n. . .\n", "a = 1\nb = 2\n").unwrap_err();
        assert!(refused[0].contains("ends before"), "{refused:?}");
    }

    #[test]
    fn an_enclosed_blank_line_takes_a_lone_marker_and_an_outside_one_takes_none() {
        let input = "a = \"\"\"\n\nb\n\"\"\"\n\nx = 1\n";
        let marked = "a = \"\"\"\n. . SSS\n\ns\nb\ns\n\"\"\"\nZZZ\n\nx = 1\n. . .\n";
        let truth = Truth::read(marked, input).unwrap();
        assert_eq!(truth.lines[1].marker, "s");
        assert_eq!(truth.lines[4].marker, "");
    }

    #[test]
    fn every_mark_of_every_kind_is_on_the_alphabet() {
        for marks in KINDS {
            for mark in [marks.opener, marks.interior, marks.closer] {
                assert!(ALPHABET.contains(mark), "{} of a {}", mark, marks.name);
                assert!(marks.owns(mark));
            }
        }
        for mark in [RESIDUE, TAG_OPENS, TAG_CLOSES] {
            assert!(ALPHABET.contains(mark), "{mark}");
            assert!(KINDS.iter().all(|marks| !marks.owns(mark)), "{mark} belongs to no span");
        }
    }

    #[test]
    fn a_span_that_closes_where_none_is_open_is_refused() {
        let refused = Truth::read("x = 1 */\n. . . UU\n", "x = 1 */\n").unwrap_err();
        assert!(refused[0].contains("closes a comment where none is open"), "{refused:?}");
        let refused = Truth::read("x = 1 \"\n. . . Z\n", "x = 1 \"\n").unwrap_err();
        assert!(refused[0].contains("closes a string where none is open"), "{refused:?}");
    }

    #[test]
    fn bytes_that_belong_to_no_open_span_are_refused() {
        let refused = Truth::read("int x = 1;\ncc. . . ..\n", "int x = 1;\n").unwrap_err();
        assert!(refused[0].contains("bytes of a comment where none is open"), "{refused:?}");
        let refused = Truth::read("/* a . b */\nCCccc.cccUU\n", "/* a . b */\n").unwrap_err();
        assert!(refused[0].contains("bytes of a comment where none is open"), "{refused:?}");
        let read = Truth::read("/* a */ x\nCCcccUU .\n", "/* a */ x\n").unwrap();
        assert_eq!(read.lines.len(), 1, "the same line with nothing left over is read");
    }

    // A pair written inside an open comment, which a language whose comments nest allows, is that
    // comment's bytes like everything else in it: marking it as a comment of its own leaves the
    // outer one closed too early, and the lines under it holding bytes that belong to nothing.
    #[test]
    fn a_comment_pair_inside_an_open_comment_is_that_comment_s_bytes() {
        let input = "/* outer\n   /* inner */\n   still inside\n*/\n";
        let marked = "/* outer\nCCcccccc\n   /* inner */\ncccCCcccccccUU\n   still inside\n\
                      ccccccccccccccc\n*/\nUU\n";
        let refused = Truth::read(marked, input).unwrap_err();
        assert!(refused[0].contains("bytes of a comment where none is open"), "{refused:?}");
        let owned = "/* outer\nCCcccccc\n   /* inner */\ncccccccccccccc\n   still inside\n\
                     ccccccccccccccc\n*/\nUU\n";
        assert!(Truth::read(owned, input).is_ok());
    }

    // Clojure writes a character as `\"`, one symbol and one byte, so a span can end where its
    // bytes do and never carry a closing symbol at all.
    #[test]
    fn a_span_with_no_closing_symbol_ends_where_its_bytes_do() {
        let read = Truth::read("(def q \\\")\n.... . Ss.\n", "(def q \\\")\n").unwrap();
        assert_eq!(read.lines.len(), 1);
        let input = "s = \"never closed\n";
        let read = Truth::read("s = \"never closed\n. . Sssssssssssss\n", input).unwrap();
        assert_eq!(read.lines.len(), 1, "and a file may end before its string does");
    }
}
