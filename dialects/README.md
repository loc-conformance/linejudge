# Dialect files

A dialect file is one counter's declaration of how it sorts the lines of a file into its own
buckets. It is the yardstick that counter is measured against: for every case, the right answer is
worked out from the case's marked strings and comments by these rules, and the counter's own output
is compared to it. Nothing else is ever used as the expectation, so writing this file is writing
down what your tool means to do.

One folder per counter and one file per way it counts, `<counter>/<dialect>.toml`, and the file's
own `counter` and `dialect` keys must match the folder and the name. The folder is the unit a
tool's maintainer owns, however many dialects it grows. A tool with one way of counting calls it
`default`; mezura counts two ways and its folder holds two files.

A dialect file carries no case names. It declares how the tool counts, knows nothing about any
corpus, and stays portable into the tool's own repository; what a tool answers on a particular
case lives in that counter's file under `recorded/`.

```toml
counter = "tokei"
dialect = "default"
buckets = ["code", "comments", "blanks"]

[counts-as-its-own-language]
rust-doc-comment = true
vue-template = true

[[rule]]
name   = "anything-outside-spans-is-code"
when   = ["has-residue"]
bucket = "code"
```

## Rules are unordered, and each condition is complete

**The rules are not tried in order. There is no first-match-wins.** A rule's `when` list is the
whole truth of when it fires: every line that meets all of its conditions goes into its bucket,
whatever the other rules say. Two rules matching the same line is legal exactly when they name the
same bucket, and that is how OR is written: two rules, same bucket, no operator. The list itself
means AND, and `!` in front of a predicate means NOT.

This costs words, "in a comment and nothing else" instead of "in a comment, if nothing above me
fired", and it buys rules that can be read and agreed one at a time, and set side by side to show
where two tools part on the same line.

Three checks stand where ordering would have been, run over the actual lines of every case:

- a line that no rule matches fails the run: the dialect is incomplete;
- a line matched by two rules with different buckets fails the run: a condition is not specific
  enough;
- a rule that decides no line of the whole corpus is reported dead, by its name.

Every rule carries a `name`, so a report can say which rule took a line and a dead rule can be
named. The name is the rule's claim, written out so it reads as a sentence: `a-string-is-code`,
`blank-outside-everything-is-blank`. A name that only points at the situation (`punctuation-only`)
or at nothing (`rule-2`) makes every report that quotes it a riddle.

## The predicates

A predicate is a yes/no question about one line, its spans and what encloses it. Delimiters belong
to their spans; residue is what is left of a line when its strings and comments are taken away; a
word is a letter, a digit, or any character above ASCII. A line with no characters at all can still
be inside a string or comment that is open across it.

- `blank`: the line holds nothing but whitespace.
- `has-residue`: a non-whitespace character outside every span.
- `in-comment`: a comment span covers part of the line, or a comment is open across it.
- `in-string`: the same for string spans.
- `word-in-comment`: a word character inside a comment span.
- `word-in-residue`: a word character outside every span.
- `in-doc-string`: the line is in a string whose opening delimiter is a tripled quote at the start
  of its own line, whitespace before it allowed.

The vocabulary is implemented once, in the engine, and shared by every dialect; a dialect file
holds no code. A new predicate lands only together with a case that uses it.

## What a dialect answers in `[counts-as-its-own-language]`

Some stretches of a file can fairly be counted either way: the markup inside a Vue `<template>` is
HTML to one tool and simply the Vue file to another, and neither is wrong. The corpus marks such a
stretch as an optional reading, and `cases/readings.toml` defines each one with a sentence and the
case that witnesses it. A dialect answers every reading it will meet: `true` counts that stretch as
a language of its own, `false` leaves its lines to the code around them. A reading left unanswered
is refused when a case marks it, rather than guessed either way.

## Deliberate behavior only

A dialect says what a tool means to do, not what it was observed doing. A rule goes in when the
behavior is documented or consistent everywhere; a difference that looks like a bug stays a
recorded failure on the cases it touches, because a rule written for every difference turns each
bug into a definition and the report can never find one again.

A deliberate, consistent behavior that no rule over spans can express can be declared as an
exception for that one case: expected counts outright, with a mandatory note, in that counter's
file under `recorded/`. The case then passes and is counted apart in every report. No counter
declares one today; the [README](../README.md) has a full example.
