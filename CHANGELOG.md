# Changelog

## 0.3.0, 2026-10-29

The corpus grows to 95 active cases and tightens 38 existing cases so that a counter cannot pass a
trap by accident.

Breaking:

- Thirty-eight existing cases had their trailing lines strengthened so that a misparsed comment,
  string or line splice changes the line counts rather than coinciding with the right answer. Some
  counters are expected to fail some existing cases that were passing by mistake.
  Regenerate recorded answers with `linejudge record --counter <name>`:
  - Unclosed comments (1060, 2040, 2140, 2150, 6020, 7020 now end in a closed comment, 1240 holds
    a second closed comment below its macro, and 2130 ends in a line comment holding `*/`).
  - Strings left open by a missed closer or escape (1130, 2050, 3010, 3020, 3030, 3050, 3070, 3080,
    3090, 4010, 4020, 4050, 4060, 4070, 4080, 4090, 6010 and 7010 open a block comment after the
    string on the same line, 2010 is rewritten in JavaScript, 2020 and 2030 end in a comment line,
    and 2080, 2110 and 2120 hold a `#` line inside the string).
  - Spliced lines (5060 now splices twice; 5010 and 5020 note that they form a pair).
  - Three redundant cases were removed in favour of the cases that test the same construct: 1050
    (covered by 1090), 1140 (covered by 1150, 8040 and 8050) and 2070 (covered by 2072).
  - Measured against the 0.2.0 records, these tightened cases catch previously masked failures in
    cloc (2010, 2040, 2110, 2120, 5060, 7020, and 2080 under `--docstring-as-code`), scc (2110) and
    mezura 3.2.0 (7010).
- In `truth.txt`, an opening HTML/Vue tag split across multiple lines must carry `>` on every line
  up to its closing `>`; leaving an intermediate line unmarked is now refused.

New:

- Fourteen new cases:
  - 1195: a C block comment ending on `assets/*/`, where `*/` closes the comment even though `*`
    follows `/`.
  - 1215: overlapping `/*/*/` in Rust and C where comment openers and closers share bytes.
  - 1260: Perl POD containing an indented `=cut` example line that does not end the block.
  - 1280: MATLAB `%{` with trailing text on the same line (a line comment, not a block opener) and
    a nested `%{ ... %}` block.
  - 2025, 2026: raw string openers inside comments or strings on a line that later closes a real
    raw string.
  - 2072, 2077: multi-line C# verbatim and interpolated verbatim strings (`@"..."`, `@$"..."`) with
    doubled `""` quotes.
  - 2085: Kotlin/Scala multi-line `"""` strings whose content ends with one or more quotes before
    the closing `"""`.
  - 2125, 2127: Ruby `class <<self` (not a heredoc) above a real heredoc, and `<<~TEXT` (indented
    closer) above `<<RUBY` (unindented closer only).
  - 2135: Rust `r##"..."##` raw strings containing a shorter `"#` sequence inside.
  - 4055: Java/Kotlin/C# `'"'` character literals containing a double quote before a comment token.
  - 6105: a Vue `<style>` opening tag split across four lines with `lang="scss"` on an inner line.
- `propose-markers` now accepts `@N` right after the opener of a crossing span (`1-4 C /*@1| ...
  |*/`), while a trailing `@N` continues to select the closer occurrence.

Other:

- mezura 3.3.0 declared (with `--no-default-config` and `--no-local`), with its answers recorded
  over all 95 cases.
- Cases 6030, 6080 and 6100 were re-marked for multi-line tags with no change to any counter's
  verdict.

-----------------------------------------------------------------------------------------------------------

## 0.2.0, 2026-09-12

A counter can now be measured more than one way against a single set of rules. cloc has
`--strip-str-comments`, which changes no rule cloc declares and changes how cloc reaches them.
Measured over the corpus it moves exactly one case, and cloc goes from 58 agree to 59 with the
flag, judged by the same five rules it already declares. cloc also gains a second set of rules.
`--docstring-as-code` moves a rule, so it is a dialect file of its own, and cloc now has three
variations over two rulesets.

Breaking:

- An adapter's `[dialect.<name>]` block is now `[variation.<name>]`, carrying a `dialect =` line
  that names the rules file judging it and `major = true` on the one variation each dialect is
  scored and badged by. A file still written the old way is refused with a message naming the
  change. A variation that writes no `read` block is read like the major of its dialect, so a flag
  that only adds arguments needs three lines and no copy of any rules.
- `data.json` renames a counter's `dialects` field to `variations`, and each entry gains `dialect`,
  `major` and `flags` beside the answers it already held. `dialect` says which variations were
  judged by one set of rules and `major` which of them is the counter's score.
- `Invocation` is now `Variation` and carries `name_of_dialect` and `is_major`.
  `Adapter::invocations` is `Adapter::variations`.
- `RecordedAnswers::read` takes the adapter whose record it is reading, replacing the counter name
  and a `Dialects`, so a record can no longer be read against another counter's rules.
- `shipped::replaces_nothing_carried_under` is `replaces_nothing_carried_of` and takes the counter
  whose files it is asked about, since a layer is now judged one counter at a time.
- `RecordedAnswers::cases_spoken_about` is replaced by `name_every_answer_block`,
  `name_every_exception_block` and `name_every_block_no_longer_declared`, which yield different
  things and used to be one stream. `find` takes a variation name and `find_exception` a dialect
  name, which is why they no longer share a parameter name.

New:

- A recorded answer that is exactly its dialect's major's writes `same-as = "<major>"`, and its
  answer, its failure flag and the sentence explaining it are all read from the block it names.
  Correcting that sentence on the major corrects it for both. A variation needing a sentence of its
  own writes a `note` beside the pointer and that one wins.
- `record` says what moved since the record it replaced: which cases started failing, which
  stopped, which blocks it is about to erase because the adapter no longer declares them, and which
  read their answer through a name that is gone. The weekly version-check job reads that, which it
  has to, since a variation written as a pointer carries no failure flag for a text search to find.
- `Adapter::variations` comes out grouped by dialect, the major of each first, so anything listing
  them shows the runs of one ruleset together.
- cloc declares `--strip-str-comments` as a second variation of its default rules, and
  `--docstring-as-code` as a ruleset of its own in `dialects/cloc/docstring-as-code.toml`. That
  file drops the doc-string rule and widens the string rule to cover every string, which is what
  the flag does inside cloc: the stage that would turn a docstring into a comment is skipped.
  Measured over the corpus it agrees on 59 of 84. Its one failure is 2090, where a line inside a
  docstring begins with a `#` and cloc's ordinary comment filter takes it.

Fixes:

- `explain` gave up on any file where cloc rewrote every line and dropped none, which is what
  `--strip-str-comments` does to case 2190. It reads it again.
- A badge is decided one counter at a time. Declaring a counter of your own used to take the badge
  off every counter in that run, the ones measured exactly the way this suite measures them
  included, and their pages then pointed at pictures nobody had drawn. A counter earns its badge
  when its adapter, its dialects and its recorded answers are what this build carries, the run
  names each counter that earns none and why, and a page draws no badge that was not written.
- A variation name and a case name are written into a key of the recorded file, so both are now
  refused unless they hold letters, digits, `_` and `-`. A name holding a space or a dot made
  `record` write a file it could not read back and exit successfully.
- Two variations of one dialect asking for it with the same command line are refused, where before
  the counter was run twice and answered twice.
- An exception declared under a dialect the counter no longer has stops the read. It is written by
  hand and nothing can write it again, so a rewrite must not drop it quietly. A stale answer key is
  still only reported, since `record` makes those again.

Other:

- mezura 3.1.1 declared, with its answers recorded. Every one of the 168 is what 3.1.0 answered.
- The walkthrough in the README runs case 2190. It ran 2160, which has since been set aside as
  disabled, so both commands it showed printed a refusal.

-----------------------------------------------------------------------------------------------------------

## 0.1.1, 2026-09-09

Other:

- scc 4.1.0 and tokei 15.0.0 declared, with their answers recorded.
- mezura gained an `[acquisition]` block, so `fetch` can build it.
- Badges written per counter, for a README to embed.
- The version-check workflow records the new build's answers, so a bump arrives with them already
  in the pull request.
- Records no longer carry answers for cases that are disabled.

-----------------------------------------------------------------------------------------------------------

## 0.1.0, 2026-09-02

The first release. A corpus of small source files with their hand-verified strings and comments, a
format in which a counter declares how it counts, and the engine that measures a counter against
its own declaration.
