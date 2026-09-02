<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo/linejudge-on-dark-512.png">
  <img src="assets/logo/linejudge-512.png" alt="" width="72">
</picture>

# LineJudge

[![CI](https://github.com/loc-conformance/linejudge/actions/workflows/ci.yml/badge.svg)](https://github.com/loc-conformance/linejudge/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/linejudge.svg)](https://crates.io/crates/linejudge)
[![docs.rs](https://docs.rs/linejudge/badge.svg)](https://docs.rs/linejudge)
[![licence](https://img.shields.io/badge/licence-MIT%20OR%20Apache--2.0-blue.svg)](#licence)

The test corpus and engine that check line-of-code counters against their own declared rules.

Two counters can read the same file and print different numbers, and usually neither of them is
wrong. Take a blank line in the middle of a block comment. One counter looks at the line itself,
sees nothing written on it, and calls it blank. Another looks at where the line stands, sees it is
inside a comment-block, and calls it a comment. Both are reasonable. Nothing can objectively decide
which one is correct.

So this project checks a counter against *itself*. Every counter
writes down the rules it uses to count, in a file anyone can read. Every test file comes with an objective record of where
each string and comment begins and ends, according to the language spec or lexer, and checked by hand.
Put those two together and you get the
right answer for that counter on that file. **A failure means one thing only: the counter did not count a line the same way it states it
should.**

Why any of this exists and what the wider project is for is at the Organization's page:
[loc-conformance](https://github.com/loc-conformance).

- [See the results](#see-the-results)
- [One case, from start to finish](#one-case-from-start-to-finish)
- [Running it](#running-it)
- [Running it against your own counter](#running-it-against-your-own-counter)
- [Adding native support for your counter here](#adding-native-support-for-your-counter-here)
- [The corpus](#the-corpus)
- [Licence](#licence)

## See the results

LineJudge publishes a webpage with every counter measured against every case:

**https://loc-conformance.github.io/linejudge/**

Each column is a counter and each row is a test file.

- **Hover a failure** and it says what went wrong.
- **Open a case** for the file itself, with its strings and comments colored in, the category each
  counter put every line in, and the command that reproduces it.
- **Open a counter** for its failures, grouped by each way it counts, and the rules it counts by.

## One case, from start to finish

Here is the whole idea in one example. A case is three files: the input being counted, a
`case.toml` with one sentence saying what trips counters up in it, and a `truth.txt` recording
where each string and comment begins and ends. This is the input file of case
`2160-long_bracket_string_holding_a_comment_symbol`:

```lua
local s = [[a string
-- not a comment, it is inside the string
]]
```

Lua writes a string that runs over several lines with `[[` and `]]`. The `--` on the second line
looks like a comment opener, but it sits inside that string, so it is text.

Beside the file sits `truth.txt`, which records where the string is. Under a copy of every line runs
one marker per character:

```
local s = [[a string
..... . . SSssssssss
-- not a comment, it is inside the string
sssssssssssssssssssssssssssssssssssssssss
]]
ZZ
```

`S` marks the characters that open the string, `s` its contents, `Z` the ones that close it, and `.`
anything outside (likewise, `C` opens a comment, `c` is its contents, and `U` closes it).  

Nothing here says how many lines of code or comments the file has. `truth.txt` just gives you the objective
bounds of the token spans, like a lexer would. How these lines get counted is open to interpretation.

Let's see an example. tokei's (14.0.0) rules say a line holding any part of a string counts as
code. The string touches all three lines here, opened on the first, filling the second, closed on
the third, so under tokei's own rules this file is three lines of code and no comments. Here is
the `explain` command in action:

```
$ linejudge explain 2160 --counter tokei

tokei.default on 2160-long_bracket_string_holding_a_comment_symbol
  by its rules    3 lines, 0 blanks, 3 code, 0 comments
  tokei answers   3 lines, 0 blanks, 2 code, 1 comments   ✗ differs
  tokei declares no per-line command of its own

  1  local s = [[a string
     ..... . . SSssssssss
     code  by anything-outside-spans-is-code and by a-string-is-code   (has-residue, in-string, word-in-residue)

  2  -- not a comment, it is inside the string
     sssssssssssssssssssssssssssssssssssssssss
     code  by a-string-is-code   (in-string)

  3  ]]
     ZZ
     code  by a-string-is-code   (in-string)
```

**tokei counted the second line as a comment. Its own rules say it is code.** That is the failure,
and no opinion of ours went into it.

In the output, you can see an analysis of every line of the test by linejudge. On the first row,
it shows the real content of the line of the input file, on the second row it shows the corresponding
symbols of the truth.txt, and on the third, linejudge infers how this line should have been counted,
and what specific rule defined in tokei's own dialect says so. 

Tokei can only hand over its totals, so that failure is found by comparing the total numbers. Some
counters (like scc and mezura, whereas cloc hints it so we can infer it) expose a way to tell you how *each individual line* is counted, and then the report names the problematic line itself:

```
$ linejudge explain 7020 --counter mezura

mezura.content on 7020-regex_holding_a_comment_opener
  by its rules    3 lines, 3 code, 0 comments, 0 extra
  mezura answers  3 lines, 1 code, 2 comments, 0 extra   ✗ differs
  mezura reads 2 lines differently

  2  let x = 1;
     ... . . ..
     code  by words-outside-spans-are-code   (has-residue, word-in-residue)
     mezura says comments
```

Line 2 is plain code and mezura called it a comment.

## Running it

Install it, download the counters it knows about, and run them all:

```
cargo install linejudge
linejudge fetch
linejudge check
```

Without cargo, take the binary for your platform from the
[latest release](https://github.com/loc-conformance/linejudge/releases/latest) and put it on your
path.

`fetch` is optional, but downloads each counter that is natively supported at exactly the version named in its adapter file. Everything it downloads goes where
this program keeps its own files. Nothing is written into your project.

`check` runs every counter it has a binary for, over every case, and prints a cli report. It answers two
separate questions.

- The first is whether a counter agrees with its own declared rules. That answer is self-contained: run the counter, derive what its rules ask, compare. For a counter without native support in the LineJudge repo, that's the only question that can be answered.

- The second is whether a counter still answers what it answered the last time we measured it. That
one needs our `recorded` snapshot of the counter, and it is judged only when the binary you are running is the
same version written at the top of that snapshot. Run a different build and the report says what
changed without calling it a regression, because a snapshot of one build says nothing about another.

Naming a case runs that one alone, which is what you want while you are fixing something:

```
linejudge check 2160 --counter tokei
linejudge explain 2160 --counter tokei
```

Any part of a name works as long as it fits exactly one case, and the run tells you which case it
picked. `explain` prints the whole derivation: what the rules ask, what the counter answered, and every
line of the file with the rule that took it.

`linejudge render` creates the webpage instead of the cli report.

## Running it against your own counter

Use the corpus in your CI, in your test suite, or by hand while you are working on your counter.
Point `.linejudge/counters.toml` at your binary, write down what it answers today, and from then on
the build breaks only when a case answers something the record does not hold:

```
linejudge record --counter mycounter
linejudge check --counter mycounter
```

`record` writes `.linejudge/recorded/mycounter.toml`, one entry per case saying what your counter
printed and, where that is not what your rules ask, that it is a failure you know about. Commit it.
A case that starts failing, or an old failure that changes its answer, breaks the run and shows the
recorded numbers beside the new ones. When you fix one, run `record` again and the diff is the list
of what your fix moved.

The run can also write a badge for your own README. The files, the record and the whole local
setup are in [FOR-COUNTER-AUTHORS.md](FOR-COUNTER-AUTHORS.md).

## Adding native support for your counter here

To be measured here and appear on the results page linked at the top, your counter needs three
things: a file saying how it counts, a file saying how to run it and how to read what it prints,
and a way for us to download it. The files, the formats and the steps are in
[FOR-COUNTER-AUTHORS.md](FOR-COUNTER-AUTHORS.md).

A counter on that page also gets a badge you can use, one per way it counts. That one shows how
the cases went:

<img src="assets/badge.png" width="198" alt="conformance: 76 agree, 1 fail and nobody has reviewed them, 8 fail">

## The corpus

The cases are grouped by what they trip up: comment pairs, string forms, escapes, quotes that open
nothing, line splices, another language inside the file, literals carrying their own delimiters,
and what a line counts as. The groups, the files of a case and the marker language are in
[cases/README.md](cases/README.md). The whole corpus ships inside the binary, so `check` needs
nothing on disk.

## Licence

The suite is MIT or Apache-2.0, at your option. The cases carry CC0 instead, in `cases/LICENSE`.
They are dedicated to the public domain, so you can copy any of them into your own repository
without asking and without carrying an attribution line with them.
