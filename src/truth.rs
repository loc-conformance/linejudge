const ALPHABET: &str = "SsCc.>< \t";
const OPTIONAL_TAG: &str = "(optional)";

/// The hand-verified spans of one case, read from its `truth.txt`: under a copy of every source
/// line, one marker character per byte, `S` and `s` for a string's delimiters and its interior,
/// `C` and `c` for a comment's, `.` for a byte outside every span, whitespace outside spans kept
/// as it stands, and a blank line that a span is open across carrying a lone `s` or `c`. A tag
/// that opens a stretch of another language is marked `>` and the tag that closes it `<`, the
/// language named after the markers of the opening line; a region with no tag is the same name
/// on the line where its span opens. Which lines belong to a region is computed, never declared.
#[derive(Debug)]
pub struct Truth {
    pub lines: Vec<TruthLine>,
}

#[derive(Debug)]
pub struct TruthLine {
    pub marker: String,
    pub region: Option<RegionClaim>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionClaim {
    pub language: String,
    pub optional: bool,
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
        if !faults.is_empty() {
            return Err(faults);
        }

        let regions = assign_regions(&lines)?;
        Ok(Truth {
            lines: lines
                .into_iter()
                .zip(regions)
                .map(|(raw, region)| TruthLine { marker: raw.marker, region })
                .collect(),
        })
    }
}

struct RawLine {
    source: String,
    marker: String,
    label: Option<RegionClaim>,
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
    if !marker.contains('>') && !marker.contains('S') && !marker.contains('C') {
        return Err(format!("[{}] labels a line that opens no tag and no span", excess.trim()));
    }
    let named = excess.trim();
    let optional = named.ends_with(OPTIONAL_TAG);
    let language = named.trim_end_matches(OPTIONAL_TAG).trim();
    if language.is_empty() || !language.chars().next().is_some_and(char::is_alphanumeric) {
        return Err(format!("[{named}] does not name a language"));
    }
    Ok((marker, Some(RegionClaim { language: language.to_string(), optional })))
}

// Regions nest, a page's script inside a fence being the everyday file, and every line belongs
// to the innermost one: the tags are a stack, and a tag's own lines belong to the region that
// encloses the tag, which for a top-level tag is the file itself.
fn assign_regions(lines: &[RawLine]) -> Result<Vec<Option<RegionClaim>>, Vec<String>> {
    let mut faults = Vec::new();
    let mut regions: Vec<Option<RegionClaim>> = lines.iter().map(|_| None).collect();

    let mut stack: Vec<RegionClaim> = Vec::new();
    let mut group: Option<char> = None;
    for (index, line) in lines.iter().enumerate() {
        let opens = line.marker.contains('>');
        let closes = line.marker.contains('<');
        if opens && closes {
            faults.push("a line both opens and closes a tag, which is not yet a shape".to_string());
            group = None;
        } else if opens {
            if group != Some('>') {
                match &line.label {
                    Some(claim) => stack.push(claim.clone()),
                    None => faults.push("an opening tag names no language".to_string()),
                }
                group = Some('>');
            } else if line.label.is_some() {
                faults.push("a second label inside one opening tag".to_string());
            }
            regions[index] = stack.get(stack.len().wrapping_sub(2)).cloned();
        } else if closes {
            if group != Some('<') {
                if stack.pop().is_none() {
                    faults.push("a closing tag closes nothing".to_string());
                }
                group = Some('<');
            }
            regions[index] = stack.last().cloned();
        } else {
            group = None;
            regions[index] = stack.last().cloned();
        }
    }
    if !stack.is_empty() {
        faults.push("an opening tag is never closed".to_string());
    }

    claim_labeled_spans(lines, &mut regions, &mut faults);
    if faults.is_empty() { Ok(regions) } else { Err(faults) }
}

// A label on a span-opening line claims the consecutive lines wholly covered by that span kind
// that either continue an open span or open a new one with the same bytes, so a `///` block ends
// where a plain `//` begins. A new span is recognised against the labeled line's opening bytes,
// which carries every symmetric closer (a docstring's `"""`) and will need the delimiter pair
// spelled out the day an asymmetric closer (`*/` at the start of its own line) wants in. Inside
// a tagged region the claim nests, the innermost language winning as everywhere; only two
// labeled spans running into each other have no meaning.
fn claim_labeled_spans(
    lines: &[RawLine],
    regions: &mut [Option<RegionClaim>],
    faults: &mut Vec<String>,
) {
    let mut labeled: Vec<bool> = lines.iter().map(|_| false).collect();
    for (index, line) in lines.iter().enumerate() {
        let Some(claim) = &line.label else { continue };
        if line.marker.contains('>') {
            continue;
        }
        let Some(kind) = line.marker.chars().find(|ch| *ch == 'S' || *ch == 'C') else {
            continue;
        };
        let signature = read_opening_bytes(line, kind);
        for (at, this) in lines.iter().enumerate().skip(index) {
            let belongs = at == index
                || (is_wholly_covered(this, kind)
                    && (begins_inside(this, kind) || read_opening_bytes(this, kind) == signature));
            if !belongs {
                break;
            }
            if labeled[at] {
                faults.push(format!("the {} label runs into another labeled span", claim.language));
                break;
            }
            regions[at] = Some(claim.clone());
            labeled[at] = true;
        }
    }
}

fn is_wholly_covered(line: &RawLine, kind: char) -> bool {
    let lower = kind.to_ascii_lowercase();
    !line.marker.is_empty() && line.marker.chars().all(|ch| ch == kind || ch == lower)
}

fn begins_inside(line: &RawLine, kind: char) -> bool {
    line.marker.starts_with(kind.to_ascii_lowercase())
}

/// The source bytes under the line's first run of uppercase marks of this kind: `///` under
/// `CCC`, `"""` under `SSS`. What tells a doc comment from the plain comment beside it.
fn read_opening_bytes(line: &RawLine, kind: char) -> String {
    line.source
        .chars()
        .zip(line.marker.chars())
        .skip_while(|(_, mark)| *mark != kind)
        .take_while(|(_, mark)| *mark == kind)
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
        let claim = truth.lines[1].region.as_ref().unwrap();
        assert_eq!(claim.language, "JavaScript");
        assert!(!claim.optional);
        assert!(truth.lines[0].region.is_none(), "the tag line belongs to the parent");
        assert!(truth.lines[2].region.is_none());
        assert!(truth.lines[3].region.is_none());
    }

    #[test]
    fn a_pair_with_nothing_between_derives_a_region_that_owns_no_line() {
        let input = "<script>\n</script>\n<p>after</p>\n";
        let marked = "<script>\n>>>>>>>> JavaScript\n</script>\n<<<<<<<<<\n<p>after</p>\n............\n";
        let truth = Truth::read(marked, input).unwrap();
        assert!(truth.lines.iter().all(|line| line.region.is_none()));
    }

    #[test]
    fn a_labeled_doc_comment_covers_its_own_lines_and_stops_at_a_plain_one() {
        let input = "/// a\n///\n// plain\nfn x() {}\n";
        let marked = "/// a\nCCCcc Markdown (optional)\n///\nCCC\n// plain\nCCcccccc\nfn x() {}\n.. . ..\n";
        let refused = Truth::read(marked, input);
        // The last marker above is deliberately short, so first prove the length gate still runs.
        assert!(refused.is_err());
        let marked = "/// a\nCCCcc Markdown (optional)\n///\nCCC\n// plain\nCCcccccc\nfn x() {}\n.. ... ..\n";
        let truth = Truth::read(marked, input).unwrap();
        let markdown = |at: usize| truth.lines[at].region.as_ref().map(|c| c.language.as_str());
        assert_eq!(markdown(0), Some("Markdown"));
        assert_eq!(markdown(1), Some("Markdown"));
        assert_eq!(markdown(2), None, "a plain // is not part of the /// block");
        assert!(truth.lines[0].region.as_ref().unwrap().optional);
    }

    #[test]
    fn a_labeled_multiline_span_carries_its_interior_and_its_symmetric_closer() {
        let input = "\"\"\"\nnotes\n\"\"\"\nx = 1\n";
        let marked = "\"\"\"\nSSS Markdown (optional)\nnotes\nsssss\n\"\"\"\nSSS\nx = 1\n. . .\n";
        let truth = Truth::read(marked, input).unwrap();
        assert!(truth.lines[1].region.is_some());
        assert!(truth.lines[2].region.is_some(), "the closing quotes match the opening bytes");
        assert!(truth.lines[3].region.is_none());
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
        let language = |at: usize| truth.lines[at].region.as_ref().map(|c| c.language.as_str());
        assert_eq!(language(0), None, "the outer tag belongs to the file");
        assert_eq!(language(1), Some("HTML"));
        assert_eq!(language(2), Some("HTML"), "the inner tag belongs to what encloses it");
        assert_eq!(language(3), Some("JavaScript"));
        assert_eq!(language(4), Some("HTML"));
        assert_eq!(language(5), Some("HTML"));
        assert_eq!(language(6), None);
    }

    #[test]
    fn a_labeled_span_inside_a_tagged_region_is_the_inner_region_of_the_two() {
        let input = "<script>\n/** doc */\n</script>\n";
        let marked = "<script>\n>>>>>>>> TypeScript\n/** doc */\nCCCcccccCC Markdown (optional)\n\
                      </script>\n<<<<<<<<<\n";
        let truth = Truth::read(marked, input).unwrap();
        let claim = truth.lines[1].region.as_ref().unwrap();
        assert_eq!(claim.language, "Markdown");
        assert!(claim.optional);
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
        let marked = "a = \"\"\"\n. . SSS\n\ns\nb\ns\n\"\"\"\nSSS\n\nx = 1\n. . .\n";
        let truth = Truth::read(marked, input).unwrap();
        assert_eq!(truth.lines[1].marker, "s");
        assert_eq!(truth.lines[4].marker, "");
    }
}
