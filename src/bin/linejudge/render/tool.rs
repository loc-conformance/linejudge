use maud::{Markup, html};

use crate::render::data::{Answer, DialectDetail, Sweep, ToolDetail, Verdict};
use crate::render::{
    BADGES_DIR, CASES_DIR, INDEX_FILE, format_as_one_line, name_the_badge_of,
    render_the_mark_of_github, wrap_the_page,
};

// A tool's page sits one directory under the root of the site.
const UP: &str = "../";
const GITHUB_HOST: &str = "github.com";

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
    wrap_the_page(&format!("{} · LineJudge", detail.name), body, UP)
}

// GitHub's mark where the counter lives on GitHub, and the plain host name where it does not,
// since a mark that is not the host's says nothing.
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
            @if host == GITHUB_HOST { (render_the_mark_of_github()) }
            span { (host) }
        }
    }
}

// Every case this counter does not simply agree on, which is the list its maintainer works
// through. A case it broke on or does not claim is here too: neither is a failure, and somebody
// would want to see both.
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
                    @let file = format!("{}.svg", name_the_badge_of(&counter.name, &dialect.name));
                    img .badge src=(format!("{UP}{BADGES_DIR}/{file}")) alt=(file);
                    span .about {
                        "does not agree on " (named.len())
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
        let answer = |name_of_case: &str, verdict: Verdict, note: Option<&str>| Answer {
            case: name_of_case.to_string(),
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
