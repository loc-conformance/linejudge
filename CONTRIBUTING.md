# Contributing

This file provides useful context and info for whoever wants to touch this repository. For what the
pieces actually are, the dialects, the adapters, the recorded answers, read
[FOR-COUNTER-AUTHORS.md](FOR-COUNTER-AUTHORS.md) 
the cases have their own page in [cases/README.md](cases/README.md), 
and the dialect file format in [dialects/README.md](dialects/README.md).

- [What CI runs](#what-ci-runs)
- [The maintenance feature](#the-maintenance-feature)
- [Releases and versioning](#releases-and-versioning)
- [Adding a case](#adding-a-case)
  - [What makes a case](#what-makes-a-case)
  - [Writing the case](#writing-the-case)
- [Adding or changing a dialect](#adding-or-changing-a-dialect)
- [Adding first-class support for a new counter](#adding-first-class-support-for-a-new-counter)
- [Re-measuring a counter](#re-measuring-a-counter)
- [What a recorded file holds](#what-a-recorded-file-holds)
- [Exceptions, and who may write one](#exceptions-and-who-may-write-one)
- [The golden file of propose-markers](#the-golden-file-of-propose-markers)

## What CI runs

Both of these, on Linux, Windows and macOS:

```
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
```

Keep the `--all-features`: the maintenance commands sit behind a feature, and without it clippy and
the tests skip them silently.

CI also fails if any file was checked out with CRLF line endings, because the corpus is compared
byte for byte. Watch out for editors that rewrite line endings on save.

## The maintenance feature

`propose-markers` and `linejudge bump-versions` only exist when built with `--features
maintenance`, and an installed copy does not have them. Both write into this repository's own
files, the marker columns of a case and the version line of an adapter, so the jobs below that use
them are done in a checkout.

## Releases and versioning

One crate carries everything: the engine, the corpus, the dialects, the adapters and the recorded
answers. A release is a snapshot of all of it, so one version number says which cases, which rules
and which records a run used.

A separate corpus crate was rejected:

- The case files are read only by this engine, so a corpus version can require a specific engine
  version, and the two numbers would need to be kept compatible by hand.
- The recorded answers are only valid for the corpus they were measured on, so they change
  together with it.
- The executable embeds everything at build time, so for its users a split changes nothing.
- A different corpus can already be used with `--corpus`.

Every release is tagged `vX.Y.Z`. Cargo does not move a `"0.x"` pin to the next minor on its own,
so a consumer's CI only changes when they raise the pin themselves. A release whose corpus changed
re-measures every recorded counter first, as [Re-measuring a counter](#re-measuring-a-counter)
describes, so the records it
ships are answers over the cases it ships.

The version is printed by `linejudge version`, at the top of the `check` report, inside the badge
and in `data.json`, and every re-measured record carries a `measured-with` line naming the
linejudge that wrote it.

## Adding a case

### What makes a case

Every case is a different way a counter can be confused into printing the wrong numbers.

An obscure missing symbol from a language representation is not one of them. This corpus is not a
coverage test, it does not test for completeness of language definitions in the counters. If the
file only goes wrong because the counter never listed a symbol, leave it out. We take for granted
that the counters have a reasonable representation of the language's symbols, but exhaustively
testing if every obscure one exists is not the job of this corpus.

Do not judge by whether any supported counter fails or passes the case. The right of a case to be
in the corpus should be independent of how the currently natively supported counters answer it.

It is important to also check that the right numbers cannot come out by accident. If a counter
passes a case, it should be because it handles it correctly, and not because it made a series of
errors that resulted in the correct answer, or because the error it made did not change the counts.
Try to construct cases where it is very hard to pass by accident.

Two files can combine to do one job. 2090 and 2100 are the same block, documentation in one and
printed data in the other, and neither shows anything alone. When cases work that way, say so in
the trap of both.

### Writing the case

First pick the directory, then the number.

The directory: the tests under `cases/` are grouped semantically and the categories are listed in
[cases/README.md](cases/README.md). Pick the one your case belongs to.

The number: cases in a group are numbered 2010, 2020, 2030, and so on, so it can allow inserting
a case between two existing ones, depending on context. Find the existing case most
similar to yours and take a free number next to it.

A case is a directory with three files:

- `input.<ext>`: the code being counted. Only the trap, no header, no explanation.
- `case.toml`: one field, the trap, a short text saying what the file is about. It ends up on the
  case's public page, so write it for someone who has never seen this repository.
- `truth.txt`: the marked spans.

`truth.txt` marks objectively, byte by byte, which parts of the input are string and which are
comment. The marker language is documented in [cases/README.md](cases/README.md). You can write
the file by hand, but the recommended way is `propose-markers`: you write a small spec file saying
where the spans are, and `propose-markers` counts the marker columns for you. The spec's notation
is on the same page. From the repository root:

```
cargo run --features maintenance --bin propose-markers -- <spec>
```

However the file was produced, check it yourself at the end. `propose-markers` does not verify
your spec: if the spec marks the wrong bytes, the `truth.txt` comes out wrong. If the language has
a lexer that can print its tokens (`python -m tokenize`, say), run it over the input to confirm
where the strings and comments really are.

Two things that come up:

- A stretch that can fairly be counted either way (the markup inside a Vue `<template>`, say) is an
  "optional reading". Define it in `cases/readings.toml` with a sentence and the case that shows
  it. Then add a line for it in the `[counts-as-its-own-language]` table of every dialect file:
  `true` if that counter counts the stretch as a separate language, `false` if it counts those lines
  as part of the surrounding file. Concretely, case 6090 is a 13 line Vue file whose `<template>`
  holds two lines, one markup and one comment. The file totals are the same under both answers:
  13 lines, 1 blank, 9 code, 3 comments. What changes is the regions of the right answer. With
  `true`, which is tokei's answer, it must hold an HTML region with those two lines, 1 code and
  1 comment. With `false`, mezura's answer, it must hold no HTML region, and the two lines count
  as plain lines of the file. The `<template>` tag lines belong to the file either way. A dialect
  file without that line fails every case that marks the reading.
- If people disagree about the spans themselves, the language itself decides: its lexer
  if it has one, its documentation if not.

A case whose directory name starts with `disabled-` is skipped: reports mention it, but no counter
is judged on it.

## Adding or changing a dialect

The file format and the predicates are documented in [dialects/README.md](dialects/README.md).
Around that:

- `cargo test` takes every line of every case and checks it against each dialect's rules. It fails
  in three situations: a line that no rule of the dialect matches (the dialect is missing a rule),
  a line that two rules put in different buckets (one of them needs a stricter condition), and a
  rule that matches no line in the whole corpus (a dead rule, named in the failure).
- A new predicate needs a case that uses it, in the same change.
- Declare only what the counter does on purpose, documented or consistent everywhere. If it looks
  like a bug or has inconsistent behavior it should be marked as a recorded failure with a note.
  The best way to figure out apart from observations, documentation of the counter, and reading
  the code, is to contact the creators of the counter to verify your interpretation of the rules.
  We don't want to misrepresent them.

## Adding first-class support for a new counter

Everything a counter needs is described in [FOR-COUNTER-AUTHORS.md](FOR-COUNTER-AUTHORS.md):
the files it brings, the ways its answers are read, the `[acquisition]` block that makes it
downloadable, and how to measure privately before the pull request. Once a counter is merged,
record its answers as described below.

## Re-measuring a counter

```
cargo run -- record --counter <name>
```

rewrites `recorded/<counter>.toml` from scratch, with the version at the top exactly as the counter
printed it. A note in that file stays as long as the answer it describes: when an answer changes,
the run drops the note and tells you which one, so you can rewrite it if it still applies.

Version bumps arrive as pull requests. A weekly job compares each pinned version against the newest
available and opens a pull request where they differ; the new recorded answers go into that same
pull request.

## What a recorded file holds

The version at the top is written exactly as the counter printed it, and compared letter for letter
with what the running binary prints. The recorded answers are held against a run only when the two
match, so a difference that comes from running another build stays out of the verdict. A counter that declares `version-flag = ""` prints no version, says `unknown
version` on both sides, and is judged against its own record all the same. Under it comes one entry
per case and per way of counting, saying what the counter printed for that case.

`is-known-failure = true` declares out loud that those numbers differ from what the rules ask. A
`note` is allowed with or without the flag: under it the note is the reason for the failure, without
it the note is anything worth recording. A flag the numbers beside it contradict is refused, so a
fix cannot leave a stale "known failure" standing. Nothing is ever refused for want of a note.

No history is kept. Two answers for one case, keyed by version, are refused for good, because
version strings are free text that nobody can put in order.

## Exceptions, and who may write one

The same file holds that counter's exceptions, in a section of their own:

```toml
[exception.2090-docstring_holding_a_comment_symbol.default]
expected = { lines = 11, code = 2, comments = 9, blanks = 0 }
note = """
its documentation detector reads the whole docstring, both symbols included, as comment on
purpose, which no rule over spans can express"""
```

An exception replaces the answer the rules would work out for that one case. The case then passes,
and it is counted separately in every report. So a counter cannot quietly declare its way to a clean
report: twenty exceptions show up as twenty exceptions, on the page and in the numbers.

**An exception is a claim about what the counter means to do, so only whoever carries responsibility
for that counter writes one.** This repository ships none, because none has been needed. The rules
have described every counter measured so far, and an exception that does get written is evidence
that the dialect model fell short somewhere, which is the first thing to look at before accepting
it.

It is the right answer for one thing only: a consistent, deliberate reading that no rule over spans
can express. It is the wrong answer for a bug, for a difference between versions, and for a case
somebody wants green. The note is mandatory and has to say which reading it is and why it is
deliberate.

## The golden file of propose-markers

The `propose-markers` tests compare its output against a stored file. Don't edit that file by
hand since every line of it is a column count, and counting columns by hand is what `propose-markers`
exists to prevent. To regenerate it:

```
LINEJUDGE_UPDATE_GOLDEN=1 cargo test --features maintenance --bin propose-markers the_golden
```

Then read the diff before committing it.
