use maud::{Markup, html};

use crate::render::data::{Answer, DialectDetail, Sweep, ToolDetail, Verdict};
use crate::render::{CASES_DIR, INDEX_FILE, format_as_one_line, wrap_the_page};

/// A tool's page sits one directory under the root of the site.
const UP: &str = "../";
const GITHUB_HOST: &str = "github.com";
/// GitHub's own mark, drawn rather than fetched, since the pages ask nothing of any other host.
const GITHUB_MARK: &str = "M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 \
    0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01\
    -.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89\
    -3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 \
    2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 \
    2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8\
    .012 8.012 0 0 0 16 8c0-4.42-3.58-8-8-8z";

pub fn render_one_tool(detail: &ToolDetail, sweep: &Sweep) -> String {
    let body = html! {
        p .crumb { a href=(format!("{UP}{INDEX_FILE}")) { "← every case" } }
        h1 {
            (detail.name)
            @if let Some(home) = &detail.repository { (render_the_link_to(home)) }
        }
        p .meta {
            "measured at " code { (detail.version) }
            @match &detail.channel {
                Some(channel) => span { " · downloaded from " (channel) },
                None => span { " · not downloaded by this suite, whoever runs it holds the binary" },
            }
        }
        h2 { "What it fails" }
        (render_the_worklist_of(detail, sweep))
        h2 { "The rules it is judged by" }
        @for dialect in &detail.dialects { (render_one_dialect(dialect)) }
    };
    wrap_the_page(&detail.name, body, UP)
}

/// The counter's own home, with GitHub's mark where that is where it lives and the plain host
/// where it is somewhere else, since a mark that is not the host's says nothing.
fn render_the_link_to(home: &str) -> Markup {
    let host = home
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(home)
        .split('/')
        .next()
        .unwrap_or(home);
    html! {
        a .home href=(home) target="_blank" rel="noreferrer" {
            @if host == GITHUB_HOST {
                svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true" {
                    path d=(GITHUB_MARK) fill="currentColor" {}
                }
            }
            span { (host) }
        }
    }
}

/// Every case this counter does not simply agree on, which is the list its own maintainer works
/// through. A case it broke on or does not support is here too: neither is a failure, and both are
/// things somebody would want to see.
fn render_the_worklist_of(detail: &ToolDetail, sweep: &Sweep) -> Markup {
    let Some(counter) = sweep.counters.iter().find(|one| one.name == detail.name) else {
        return html! {};
    };
    html! {
        @for dialect in &counter.dialects {
            @let named: Vec<&Answer> = dialect
                .answers
                .iter()
                .filter(|answer| answer.verdict != Verdict::Agrees)
                .collect();
            div .worklist {
                p .heading {
                    @if counter.dialects.len() > 1 {
                        span .way { (dialect.name) }
                    }
                    span .about {
                        "does not simply agree on " (named.len())
                        " of " (dialect.answers.len()) " cases"
                    }
                }
                @if named.is_empty() {
                    p .note { "it answers every case the way its own rules ask" }
                }
                @for answer in named {
                    div .item {
                        a href=(format!("{UP}{CASES_DIR}/{}.html", answer.case)) { (answer.case) }
                        " " (render_the_verdict_of(answer))
                        @if let Some(note) = &answer.note {
                            span .note { (format_as_one_line(note)) }
                        }
                    }
                }
            }
        }
    }
}

fn render_the_verdict_of(answer: &Answer) -> Markup {
    match answer.verdict {
        Verdict::Fails if answer.note.is_some() => html! { span .status.s-fail { "✗" } },
        Verdict::Fails => html! { span .status.s-open { "✗ unreviewed" } },
        Verdict::Unclaimed => html! { span .status.s-na { "⊘ not supported" } },
        Verdict::Broke => html! { span .status.s-broke { "broke" } },
        Verdict::Agrees => html! {},
    }
}

fn render_one_dialect(dialect: &DialectDetail) -> Markup {
    html! {
        div .dialect {
            p .heading {
                span .way { (dialect.name) }
                @match dialect.flags.is_empty() {
                    true => span .about { "how it counts when it is run with no extra flags" },
                    false => span .about {
                        "how it counts when it is run with " code { (dialect.flags.join(" ")) }
                    },
                }
            }
            table .rules {
                @for rule in &dialect.rules {
                    tr {
                        td .bucket-of { (rule.bucket) }
                        td {
                            div .rule-name { (rule.name) }
                            @for asked in &rule.when {
                                div .asks { (asked) }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use crate::render::data::{Counter, Counts, Dialect, RuleDetail};

    use super::*;

    #[test]
    fn the_worklist_holds_every_case_it_does_not_simply_agree_on() {
        let detail = a_tool();
        let shown = render_the_worklist_of(&detail, &a_sweep()).into_string();
        assert!(shown.contains("1 of 3"), "one of three is not agreed\n{shown}");
        // A tool's page is one directory down, so a case is reached by climbing out of it first.
        assert!(shown.contains("href=\"../cases/0500-a_failure.html\""), "{shown}");
        assert!(!shown.contains("0400-a_pass"), "an agreeing case earns no line\n{shown}");
        assert!(shown.contains("the /* opens a comment"), "{shown}");
    }

    #[test]
    fn a_dialect_shows_its_rules_in_words_and_nothing_else() {
        let shown = render_one_dialect(&a_tool().dialects[0]).into_string();
        assert!(shown.contains("part of the line is inside a comment"), "{shown}");
        assert!(shown.contains("comments"), "the bucket a rule counts into\n{shown}");
        assert!(!shown.contains("in-comment&"), "not the token from the file\n{shown}");
    }

    #[test]
    fn a_way_of_counting_is_named_beside_the_command_line_that_asks_for_it() {
        let named = render_one_dialect(&a_tool().dialects[0]).into_string();
        assert!(named.contains(">default<"), "{named}");
        assert!(named.contains("run with <code>--mode default</code>"), "{named}");

        let mut plain = a_tool();
        plain.dialects[0].flags.clear();
        let shown = render_one_dialect(&plain.dialects[0]).into_string();
        assert!(shown.contains(">default<"), "{shown}");
        assert!(shown.contains("run with no extra flags"), "{shown}");
    }

    fn a_tool() -> ToolDetail {
        ToolDetail {
            name: "tokei".to_string(),
            version: "tokei 14.0.0".to_string(),
            repository: Some("https://github.com/XAMPPRocky/tokei".to_string()),
            channel: Some("crates-io as tokei".to_string()),
            dialects: vec![DialectDetail {
                name: "default".to_string(),
                flags: vec!["--mode".to_string(), "default".to_string()],
                rules: vec![RuleDetail {
                    name: "a-comment-alone-is-comments".to_string(),
                    bucket: "comments".to_string(),
                    when: vec![
                        "part of the line is inside a comment".to_string(),
                        "no part of the line is inside a string".to_string(),
                    ],
                }],
            }],
        }
    }

    fn a_sweep() -> Sweep {
        let answer = |case: &str, verdict: Verdict, note: Option<&str>| Answer {
            case: case.to_string(),
            verdict,
            wants: Some(Counts { lines: 1, buckets: BTreeMap::new() }),
            answered: None,
            wants_regions: Vec::new(),
            answered_regions: Vec::new(),
            note: note.map(str::to_string),
            exception: None,
            broke: None,
            command: String::new(),
        };
        Sweep {
            measured_on: "2026-08-20".to_string(),
            groups: Vec::new(),
            counters: vec![Counter {
                name: "tokei".to_string(),
                version: "tokei 14.0.0".to_string(),
                dialects: vec![Dialect {
                    name: "default".to_string(),
                    answers: vec![
                        answer("0400-a_pass", Verdict::Agrees, None),
                        answer("0500-a_failure", Verdict::Fails, Some("the /* opens a comment")),
                        answer("0600-another_pass", Verdict::Agrees, None),
                    ],
                }],
            }],
        }
    }
}
