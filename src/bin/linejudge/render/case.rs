use maud::{Markup, html};

use crate::marks::Ink;
use crate::render::data::{Answer, CaseDetail, Counts, Region, Sweep, Verdict};
use crate::render::{INDEX_FILE, format_as_one_line, format_the_group_title, wrap_the_page};

// A case's page sits one directory under the root of the site.
const UP: &str = "../";
const COPY_MARK_BACK: &str = "M0 6.75C0 5.784.784 5 1.75 5h1.5a.75.75 0 0 1 0 1.5h-1.5a.25.25 0 0 \
    0-.25.25v7.5c0 .138.112.25.25.25h7.5a.25.25 0 0 0 .25-.25v-1.5a.75.75 0 0 1 1.5 0v1.5A1.75 \
    1.75 0 0 1 9.25 16h-7.5A1.75 1.75 0 0 1 0 14.25Z";
const COPY_MARK_FRONT: &str = "M5 1.75C5 .784 5.784 0 6.75 0h7.5C15.216 0 16 .784 16 1.75v7.5A1.75 \
    1.75 0 0 1 14.25 11h-7.5A1.75 1.75 0 0 1 5 9.25Zm1.75-.25a.25.25 0 0 0-.25.25v7.5c0 \
    .138.112.25.25.25h7.5a.25.25 0 0 0 .25-.25v-7.5a.25.25 0 0 0-.25-.25Z";

pub fn render_one_case(detail: &CaseDetail, sweep: &Sweep) -> String {
    let answers = find_the_answers_to(detail, sweep);
    let body = html! {
        p .crumb {
            a href=(format!("{UP}{INDEX_FILE}")) { "← every case" }
            " · " (format_the_group_title(&detail.group))
        }
        h1 { (detail.name) }
        p .trap { (format_as_one_line(&detail.trap)) }
        (render_the_file(detail))
        h2 { "What each tool answered" }
        @for (way, answer) in &answers { (render_one_answer(way, answer)) }
    };
    wrap_the_page(&format!("{} · LineJudge", detail.name), body, UP)
}

fn render_the_file(detail: &CaseDetail) -> Markup {
    let width = detail.lines.len().to_string().len();
    html! {
        div .chips .ways {
            span .picked { "read as" }
            @for (at, way) in detail.ways.iter().enumerate() {
                span .chip .pick .active[at == 0] data-group="way" data-value=(way) { (way) }
            }
        }
        table .file {
            tr .filename { td colspan="3" {
                div .bar {
                    span { (detail.file) }
                    button .copy type="button" title="copy the file" {
                        svg width="14" height="14" viewBox="0 0 16 16" aria-hidden="true" {
                            path d=(COPY_MARK_BACK) fill="currentColor" {}
                            path d=(COPY_MARK_FRONT) fill="currentColor" {}
                        }
                        span .said { "copy" }
                    }
                }
            } }
            @for (at, line) in detail.lines.iter().enumerate() {
                tr {
                    td .ln { (format!("{:>width$}", at + 1)) }
                    td .gut {
                        @for (which, counted) in line.counted.iter().enumerate() {
                            span .dv data-group="way" data-value=(detail.ways[which])
                                    hidden[which > 0] {
                                span .bucket title=(name_the_rules_of(counted)) { (counted.bucket) }
                                @if let Some(region) = &counted.region {
                                    " " span .rgn { (region) }
                                }
                            }
                        }
                    }
                    td .src {
                        @for piece in &line.pieces {
                            span class=(name_the_ink(piece.ink)) { (piece.text) }
                        }
                    }
                }
            }
        }
        p .file-legend {
            span { span .ink-string { "string" } }
            span { span .ink-comment { "comment" } }
            span { span .ink-tag { "the tag around another language" } }
        }
    }
}

fn render_one_answer(way: &str, answer: &Answer) -> Markup {
    html! {
        div .answer {
            div .who {
                span .way { (way) } " " (render_the_verdict_of(answer))
            }
            @if let Some(note) = &answer.note {
                p .note { (format_as_one_line(note)) }
            }
            @if let Some(exception) = &answer.exception {
                p .note { "declared intent: " (format_as_one_line(exception)) }
            }
            @if let Some(broke) = &answer.broke {
                p .note { (format_as_one_line(broke)) }
            }
            @if let Some(wants) = &answer.wants {
                div .nums {
                    span .lbl { "the rules ask" } (format_the_counts(wants, None))
                    @if !answer.wants_regions.is_empty() {
                        " · " (format_the_regions(&answer.wants_regions))
                    }
                    br;
                    span .lbl { "it answers" }
                    @match &answer.answered {
                        Some(answered) => {
                            (format_the_counts(answered, Some(wants)))
                            @if !answer.answered_regions.is_empty() {
                                " · " (format_the_regions(&answer.answered_regions))
                            }
                        }
                        None => span .off { "nothing, it does not support this language" },
                    }
                }
            }
            p .cmd { code { (answer.command) } }
        }
    }
}

fn render_the_verdict_of(answer: &Answer) -> Markup {
    match answer.verdict {
        Verdict::Agrees if answer.exception.is_some() => html! {
            span .status.s-exc { "◆ agrees through its exception" }
        },
        Verdict::Agrees => html! { span .status.s-pass { "✓ agrees" } },
        Verdict::Fails if answer.note.is_some() => html! { span .status.s-fail { "✗ fails" } },
        Verdict::Fails => html! { span .status.s-open { "✗ fails · unreviewed" } },
        Verdict::Unclaimed => html! { span .status.s-na { "⊘ does not support this language" } },
        Verdict::Broke => html! { span .status.s-broke { "broke" } },
    }
}

// Where another answer is given to hold these against, the numbers that differ are painted.
fn format_the_counts(counts: &Counts, against: Option<&Counts>) -> Markup {
    let differs = |name: &str, value: u32| {
        against.is_some_and(|other| other.buckets.get(name) != Some(&value))
    };
    html! {
        span .off[against.is_some_and(|other| other.lines != counts.lines)] {
            (counts.lines) " lines"
        }
        @for (name, value) in &counts.buckets {
            " · "
            span .off[differs(name, *value)] { (value) " " (name) }
        }
    }
}

fn format_the_regions(regions: &[Region]) -> Markup {
    html! {
        @for (at, region) in regions.iter().enumerate() {
            @if at > 0 { " · " }
            span .rgn { (region.language) } " " (region.lines)
        }
    }
}

fn find_the_answers_to<'a>(detail: &CaseDetail, sweep: &'a Sweep) -> Vec<(String, &'a Answer)> {
    let mut found = Vec::new();
    for counter in &sweep.counters {
        for dialect in &counter.dialects {
            let way = format!("{}.{}", counter.name, dialect.name);
            if let Some(answer) = dialect.answers.iter().find(|one| one.case == detail.name) {
                found.push((way, answer));
            }
        }
    }
    found
}

fn name_the_rules_of(counted: &crate::render::data::Counted) -> String {
    match counted.rules.is_empty() {
        true => "no rule took this line".to_string(),
        false => format!("by {}", counted.rules.join(" and by ")),
    }
}

fn name_the_ink(ink: Ink) -> &'static str {
    match ink {
        Ink::Comment => "ink-comment",
        Ink::String => "ink-string",
        Ink::Tag => "ink-tag",
        Ink::Plain => "ink-plain",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::render::data::{Counted, Line, Piece};

    use super::*;

    #[test]
    fn the_file_is_painted_by_what_covers_it_and_keeps_every_character() {
        let detail = a_case();
        let shown = render_the_file(&detail).into_string();
        assert!(shown.contains("<span class=\"ink-string\">&quot;one&quot;</span>"), "{shown}");
        assert!(shown.contains("<span class=\"ink-comment\">// two</span>"), "{shown}");
        assert!(shown.contains("title=\"by a-rule and by another-rule\""), "{shown}");
        assert!(shown.contains("Markdown"), "the region of a line is named\n{shown}");
    }

    #[test]
    fn only_the_first_way_of_counting_is_shown_and_the_rest_wait_behind_it() {
        let detail = a_case();
        let shown = render_the_file(&detail).into_string();
        assert!(shown.contains("data-value=\"mezura.content\">"), "{shown}");
        assert!(shown.contains("data-value=\"tokei.default\" hidden>"), "{shown}");
        assert_eq!(shown.matches("chip pick").count(), 2);
    }

    fn a_case() -> CaseDetail {
        CaseDetail {
            name: "0400-a_case".to_string(),
            group: "0000-a_group".to_string(),
            trap: "a trap".to_string(),
            file: "input.c".to_string(),
            ways: vec!["mezura.content".to_string(), "tokei.default".to_string()],
            lines: vec![Line {
                pieces: vec![
                    Piece { ink: Ink::Plain, text: "a = ".to_string() },
                    Piece { ink: Ink::String, text: "\"one\"".to_string() },
                    Piece { ink: Ink::Plain, text: "; ".to_string() },
                    Piece { ink: Ink::Comment, text: "// two".to_string() },
                ],
                counted: vec![
                    Counted {
                        bucket: "code".to_string(),
                        rules: vec!["a-rule".to_string(), "another-rule".to_string()],
                        region: Some("Markdown".to_string()),
                    },
                    Counted {
                        bucket: "comments".to_string(),
                        rules: vec!["a-rule".to_string()],
                        region: None,
                    },
                ],
            }],
        }
    }

    #[test]
    fn an_answer_shows_every_count_and_paints_the_ones_that_moved() {
        let counts = |code: u32, comments: u32| Counts {
            lines: 3,
            buckets: BTreeMap::from([
                ("code".to_string(), code),
                ("comments".to_string(), comments),
            ]),
        };
        let answer = Answer {
            case: "0400-a_case".to_string(),
            verdict: Verdict::Fails,
            wants: Some(counts(3, 0)),
            answered: Some(counts(1, 2)),
            wants_regions: Vec::new(),
            answered_regions: Vec::new(),
            note: Some("the /* opens a comment".to_string()),
            exception: None,
            broke: None,
            command: "tokei cases/0000-a_group/0400-a_case/input.c".to_string(),
        };
        let shown = render_one_answer("tokei.default", &answer).into_string();
        assert!(shown.contains("✗ fails"), "{shown}");
        assert!(shown.contains("3 lines"), "the lines are shown even where they agree\n{shown}");
        assert!(shown.contains("<span class=\"off\">1 code</span>"), "{shown}");
        assert!(shown.contains("the /* opens a comment"), "{shown}");
        assert!(shown.contains("<code>tokei cases/"), "{shown}");
    }
}
