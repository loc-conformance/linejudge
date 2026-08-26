# Contributing

This file tells you which command does each job. For what the pieces actually are, the corpus, the
dialects, the adapters, the recorded answers, read the [README](README.md); the dialect file format
has its own page in [dialects/README.md](dialects/README.md).

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

`propose-markers` and `linejudge record` only exist when built with `--features maintenance`, and
no installed copy has them. The jobs below that use them are done in a checkout of this repository.

## Adding a case

First pick the directory, then the number.

The directory: the tests under `cases/` are grouped semantically and the categories are listed in the README.
Pick the one your case belongs to.

The number: cases in a group are numbered 2010, 2020, 2030, and so on, so it can allow inserting 
a case between two existing ones, depending on context. Find the existing case most
similar to yours and take a free number next to it.

A case is a directory with three files:

- `input.<ext>`: the code being counted. Only the trap, no header, no explanation.
- `case.toml`: one field, the trap, a short text saying what the file is about. It ends up on the
  case's public page, so write it for someone who has never seen this repository.
- `truth.txt`: the marked spans.

`truth.txt` marks objectively, byte by byte, which parts of the input are string and which are comment. 
The marker language is documented in [cases/README.md](cases/README.md). You can write the file by hand,
 but the recommended way is `propose-markers`: you write a small spec file saying where the spans are,
and `propose-markers` counts the marker columns for you. The spec's notation is on the same page. From the repository root:

```
cargo run --features maintenance --bin propose-markers -- <spec>
```

However the file was produced, check it yourself at the end. `propose-markers` does not verify your spec:
if the spec marks the wrong bytes, the `truth.txt` comes out wrong.
 If the language has a lexer that can print its tokens (`python -m tokenize`, say), run it
over the input to confirm where the strings and comments really are.


Two things that come up:

- A stretch that can fairly be counted either way (the markup inside a Vue `<template>`, say) is an
  "optional reading". Define it in `cases/readings.toml` with a sentence and the case that shows
  it. Then add a line for it in the `[counts-as-its-own-language]` table of every dialect file:
  `true` if that counter counts the stretch as a separate language, `false` if it counts those lines
  as part of the surrounding file. A dialect file without that line fails every case that marks
  the reading.
- If people disagree about the spans themselves, the language decides, never a counter: its lexer
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
  like a bug, leave it as a recorded failure with a note instead of writing a rule around it.
- An exception says: for this one case, expect these exact counts instead of what the rules
  derive. It lives in `recorded/<counter>.toml` with a mandatory note, and the case then passes
  but is counted separately in every report. Use one only for behavior that is deliberate and
  consistent but impossible to write as a rule over spans using the predicates: bugs and
  unintentional behavior must not be turned into exceptions, so expect this to be rare. A
  justified exception also documents a limit of the dialect model, a deliberate behavior the
  rules could not express. The [README](README.md) has a full example.

## Adding first-class support for a new counter

To be merged here, a counter needs three things: an adapter, `adapters/<counter>.toml`, a dialect
folder, `dialects/<counter>/`, and an `[acquisition]` block inside the adapter, naming where the
binary is downloaded from and at which version. The adapter says how to run the counter and how to
read what it prints, either a `read` block over the counter's own JSON or a wrapper that prints the
uniform format; both are described in [docs/counter-authors.md](docs/counter-authors.md).

The `[acquisition]` block should be provided because without it nobody here can download the counter: we
could not check what we are merging, could not re-measure it when it releases, and the automated
runs that build the public page would leave it out. If your counter is distributed in a way the
existing download channels do not cover, open an issue about adding a channel instead of leaving
the block out.

A counter that cannot be downloaded at all still gets everything except the public page, from its own
side: declare it through a `.linejudge` folder in your own repository and measure it there, as the
README describes. The same path also works before merging: measure privately for as long as you
want, and the pull request that adds the dialect just makes it public. Once it is merged, record
the counter's answers as described below.

## Re-measuring a counter

```
cargo run --features maintenance -- record --counter <name>
```

rewrites `recorded/<counter>.toml` from scratch, with the version at the top exactly as the counter
printed it. A note in that file stays as long as the answer it describes: when an answer changes,
the run drops the note and tells you which one, so you can rewrite it if it still applies.

Version bumps arrive as pull requests. A weekly job compares each pinned version against the newest
available and opens a pull request where they differ; the new recorded answers go into that same
pull request.

## What a recorded file holds

The version at the top is written exactly as the counter printed it, and never parsed. Under it comes
one entry per case and per way of counting, saying what the counter printed for that case.

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
for that counter writes one.** This repository ships none. tokei's and scc's divergences stay known
failures with a note, because their intent is not ours to claim.

It is the right answer for one thing only: a consistent, deliberate reading that no rule over spans
can express. It is the wrong answer for a bug, for a difference between versions, and for a case
somebody wants green. The note is mandatory and has to say which reading it is and why it is
deliberate.

## The golden file of propose-markers

The `propose-markers` tests compare its output against a stored file. Never edit that file by
hand; every line of it is a column count, and counting columns by hand is what `propose-markers` exists to
prevent. To regenerate it:

```
LINEJUDGE_UPDATE_GOLDEN=1 cargo test --features maintenance --bin propose-markers the_golden
```

Then read the diff before committing it.
