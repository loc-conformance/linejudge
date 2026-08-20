use linejudge::truth::{COMMENT_MARKS, RESIDUE, STRING_MARKS, TAG_CLOSES, TAG_OPENS};

/// What covers a stretch of a line, as the marks under it say. Both the terminal and the pages
/// paint by this, so the alphabet is read in one place.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ink {
    Comment,
    String,
    Tag,
    Plain,
}

impl Ink {
    pub fn of(mark: char) -> Ink {
        match mark {
            _ if STRING_MARKS.owns(mark) => Ink::String,
            _ if COMMENT_MARKS.owns(mark) => Ink::Comment,
            TAG_OPENS | TAG_CLOSES => Ink::Tag,
            _ => Ink::Plain,
        }
    }
}

/// The line cut where what covers it changes, keeping every character of the source. It is walked
/// by character rather than by byte, which it may be because a case input is ASCII.
pub fn cut_into_stretches(source: &str, marker: &str) -> Vec<(Ink, String)> {
    let marks: Vec<char> = marker.chars().collect();
    let mut stretches: Vec<(Ink, String)> = Vec::new();
    for (at, letter) in source.chars().enumerate() {
        let ink = Ink::of(marks.get(at).copied().unwrap_or(RESIDUE));
        match stretches.last_mut() {
            Some((last, text)) if *last == ink => text.push(letter),
            _ => stretches.push((ink, letter.to_string())),
        }
    }
    stretches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_cut_where_what_covers_it_changes_and_keeps_every_character() {
        let source = "a = \"one\"; // two";
        let cut = cut_into_stretches(source, "... SsssZ. CCcccc");
        let rebuilt: String = cut.iter().map(|(_, text)| text.as_str()).collect();
        assert_eq!(rebuilt, source);
        let inks: Vec<Ink> = cut.iter().map(|(ink, _)| *ink).collect();
        assert_eq!(inks, [Ink::Plain, Ink::String, Ink::Plain, Ink::Comment]);
    }

    #[test]
    fn a_line_with_no_marks_under_it_is_one_plain_stretch() {
        let cut = cut_into_stretches("int x = 1;", "");
        assert_eq!(cut.len(), 1);
        assert_eq!(cut[0], (Ink::Plain, "int x = 1;".to_string()));
    }
}
