use crate::render::MARK_LINES;
use crate::render::StateCounts;
use crate::render::data::Answer;

const HEIGHT: usize = 20;
const LABEL: &str = "conformance";
const LABEL_WIDTH: usize = 90;
const SELF_RUN_LABEL: &str = "linejudge";
const SELF_RUN_LABEL_WIDTH: usize = 74;
// A block is this wide whatever its number, so the blocks of one badge are even. Only a single
// digit gets a narrower one, which would otherwise swim in its own color.
const BLOCK: usize = 40;
const NARROW_BLOCK: usize = 34;
// The self-run block carries words rather than a digit, so its width is measured off the text.
const PER_CHARACTER: usize = 7;
const AROUND_THE_WORDS: usize = 14;

// One badge for one way of counting: the label, then a block per state, each carrying its count. A
// block of nought is left out, apart from the green one, which is what the badge is for and stays.
pub fn render_one_badge(answers: &[Answer]) -> String {
    let counted = StateCounts::of(answers);
    let blocks: Vec<(usize, &str, &str)> = [
        (counted.agrees, "✓", "#2ea043"),
        (counted.open, "?", "#c99a06"),
        (counted.fails, "✗", "#cf222e"),
        (counted.unclaimed, "⊘", "#8c959f"),
        (counted.broke, "!", "#8b1a1a"),
    ]
    .into_iter()
    .enumerate()
    .filter(|(at, (count, _, _))| *at == 0 || *count > 0)
    .map(|(_, block)| block)
    .collect();

    let drawn: Vec<(String, usize, &str)> = blocks
        .iter()
        .map(|(count, symbol, color)| {
            let width = match *count < 10 {
                true => NARROW_BLOCK,
                false => BLOCK,
            };
            (format!("{count} {symbol}"), width, *color)
        })
        .collect();
    let said: Vec<String> = blocks
        .iter()
        .map(|(count, symbol, _)| format!("{count} {}", name_of(symbol)))
        .collect();
    build_the_svg(LABEL, LABEL_WIDTH, &drawn, &said.join(", "))
}

// How many cases a counter was held against, and no verdict, since the rules it was judged by are
// its author's own and nobody has reviewed them. The blue is the mark's, and is none of the five
// colors a verdict is painted in.
pub fn render_the_self_run_badge(cases: usize) -> String {
    let words = format!("{cases} cases, v{}", crate::VERSION);
    let width = words.len() * PER_CHARACTER + AROUND_THE_WORDS;
    let said = format!("measured against {cases} cases by linejudge {}", crate::VERSION);
    build_the_svg(SELF_RUN_LABEL, SELF_RUN_LABEL_WIDTH, &[(words, width, MARK_LINES)], &said)
}

// Both badges are this drawing, so the frame, the font and the rounding are decided once. `said`
// is what somebody hearing the badge read out gets instead of seeing it.
fn build_the_svg(
    label: &str,
    label_width: usize,
    blocks: &[(String, usize, &str)],
    said: &str,
) -> String {
    let mut boxes = String::new();
    let mut words = String::new();
    let mut at = label_width;
    for (text, width, color) in blocks {
        boxes.push_str(&format!(
            "<rect x=\"{at}\" width=\"{width}\" height=\"{HEIGHT}\" fill=\"{color}\"/>"
        ));
        words.push_str(&format!("<text x=\"{}\" y=\"14\">{text}</text>", at + width / 2));
        at += width;
    }
    let middle = label_width / 2;
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{at}\" height=\"{HEIGHT}\" role=\"img\" \
         aria-label=\"{label}: {said}\">\
         <clipPath id=\"r\"><rect width=\"{at}\" height=\"{HEIGHT}\" rx=\"3\" fill=\"#fff\"/></clipPath>\
         <g clip-path=\"url(#r)\">\
         <rect width=\"{label_width}\" height=\"{HEIGHT}\" fill=\"#555\"/>{boxes}</g>\
         <g fill=\"#fff\" font-family=\"Verdana,DejaVu Sans,sans-serif\" font-size=\"11\" \
         text-anchor=\"middle\">\
         <text x=\"{middle}\" y=\"14\">{label}</text>{words}</g></svg>\n"
    )
}

// What a symbol means, for whoever hears the badge read out instead of seeing it.
fn name_of(symbol: &str) -> &'static str {
    match symbol {
        "✓" => "agree",
        "?" => "fail and nobody has reviewed them",
        "✗" => "fail",
        "⊘" => "in a language it does not support",
        _ => "broke it",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::render::data::{Counts, Verdict};

    use super::*;

    #[test]
    fn a_state_a_counter_is_not_in_takes_no_block_and_the_green_one_stays_regardless() {
        let plain = render_one_badge(&[answer(Verdict::Agrees, None), answer(Verdict::Agrees, None)]);
        assert!(plain.contains(">2 ✓<"), "{plain}");
        assert!(!plain.contains('⊘'), "nothing was unclaimed, so no grey block\n{plain}");
        assert!(!plain.contains('✗'), "nothing failed, so no red block\n{plain}");

        let nothing = render_one_badge(&[]);
        assert!(nothing.contains(">0 ✓<"), "the green block stays at nought\n{nothing}");
    }

    #[test]
    fn every_state_earns_its_own_block_and_a_reviewed_failure_is_not_an_unreviewed_one() {
        let badge = render_one_badge(&[
            answer(Verdict::Agrees, None),
            answer(Verdict::Fails, Some("a note")),
            answer(Verdict::Fails, None),
            answer(Verdict::Unclaimed, None),
            answer(Verdict::Broke, None),
        ]);
        for said in [">1 ✓<", ">1 ?<", ">1 ✗<", ">1 ⊘<", ">1 !<"] {
            assert!(badge.contains(said), "{said} is missing\n{badge}");
        }
        assert!(badge.contains("1 agree, 1 fail and nobody has reviewed them, 1 fail"), "{badge}");
    }

    #[test]
    fn the_self_run_badge_says_how_many_cases_and_never_how_they_went() {
        let badge = render_the_self_run_badge(84);
        assert!(badge.contains(&format!(">84 cases, v{}<", crate::VERSION)), "{badge}");
        assert!(badge.contains(">linejudge<"), "{badge}");
        assert!(
            badge.contains(&format!(
                "aria-label=\"linejudge: measured against 84 cases by linejudge {}\"",
                crate::VERSION
            )),
            "{badge}"
        );
        for verdict in ['✓', '?', '✗', '⊘', '!'] {
            assert!(!badge.contains(verdict), "{verdict} is a verdict and has no place here\n{badge}");
        }
        for color in ["#2ea043", "#c99a06", "#cf222e"] {
            assert!(!badge.contains(color), "{color} paints a verdict\n{badge}");
        }
    }

    // The count reaches three digits the day the corpus does, and a fixed block would clip it.
    #[test]
    fn the_self_run_block_grows_with_the_number_inside_it() {
        let narrow = render_the_self_run_badge(9);
        let wide = render_the_self_run_badge(1084);
        let width_of = |svg: &str| {
            svg.split_once("width=\"").unwrap().1.split_once('"').unwrap().0.parse::<usize>().unwrap()
        };
        assert!(width_of(&wide) > width_of(&narrow), "{narrow}\n{wide}");
    }

    fn answer(verdict: Verdict, note: Option<&str>) -> Answer {
        Answer {
            case: "0400-a_case".to_string(),
            verdict,
            wants: Some(Counts { lines: 1, buckets: BTreeMap::new() }),
            answered: None,
            wants_regions: Vec::new(),
            answered_regions: Vec::new(),
            note: note.map(str::to_string),
            exception: None,
            broke: None,
            command: String::new(),
        }
    }
}
