use crate::render::StateCounts;
use crate::render::data::Answer;

const HEIGHT: usize = 20;
const LABEL: &str = "conformance";
const LABEL_WIDTH: usize = 90;
// A block is this wide whatever its number, so the blocks of one badge are even. Only a single
// digit gets a narrower one, which would otherwise swim in its own colour.
const BLOCK: usize = 40;
const NARROW_BLOCK: usize = 34;

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

    let mut boxes = String::new();
    let mut words = String::new();
    let mut at = LABEL_WIDTH;
    for (count, symbol, colour) in &blocks {
        let width = match *count < 10 {
            true => NARROW_BLOCK,
            false => BLOCK,
        };
        boxes.push_str(&format!(
            "<rect x=\"{at}\" width=\"{width}\" height=\"{HEIGHT}\" fill=\"{colour}\"/>"
        ));
        words.push_str(&format!(
            "<text x=\"{}\" y=\"14\">{count} {symbol}</text>",
            at + width / 2
        ));
        at += width;
    }
    let said: Vec<String> = blocks
        .iter()
        .map(|(count, symbol, _)| format!("{count} {}", name_of(symbol)))
        .collect();
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{at}\" height=\"{HEIGHT}\" role=\"img\" \
         aria-label=\"{LABEL}: {}\">\
         <clipPath id=\"r\"><rect width=\"{at}\" height=\"{HEIGHT}\" rx=\"3\" fill=\"#fff\"/></clipPath>\
         <g clip-path=\"url(#r)\">\
         <rect width=\"{LABEL_WIDTH}\" height=\"{HEIGHT}\" fill=\"#555\"/>{boxes}</g>\
         <g fill=\"#fff\" font-family=\"Verdana,DejaVu Sans,sans-serif\" font-size=\"11\" \
         text-anchor=\"middle\">\
         <text x=\"{}\" y=\"14\">{LABEL}</text>{words}</g></svg>\n",
        said.join(", "),
        LABEL_WIDTH / 2,
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
