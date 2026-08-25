<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo/linejudge-on-dark-512.png">
  <img src="assets/logo/linejudge-512.png" alt="" width="72">
</picture>

# LineJudge

The test corpus and harness that check line-of-code counters against their own declared rules.

Two counters can read the same file and print different numbers, and usually neither of them is
wrong. Take a blank line in the middle of a block comment. One counter looks at the line itself,
sees nothing written on it, and calls it blank. Another looks at where the line stands, sees it is
inside a comment-block, and calls it a comment. Both are reasonable. Nothing can objectively decide
which one is correct.

So this project checks a counter against *itself*. Every counter
writes down how it counts, in a file anyone can read. Every test file comes with an objective record of where
each string and comment begins and ends, according to the language spec or lexer, and checked by hand.
Put those two together and you get the
right answer for that counter on that file. **A failure means one thing only: the counter did not do what
it says it does.**

Why any of this exists and what the wider project is for is at the Organization's page:
[loc-conformance](https://github.com/loc-conformance).

## See the results

The suite publishes a page with every counter measured against every case:

**https://loc-conformance.github.io/linejudge/**

Each column is a counter and each row is a test file.

- **Hover a failure** and it says what went wrong.
- **Open a case** for the file itself, with its strings and comments colored in, the category each
  counter put every line in, and the command that reproduces it.
- **Open a counter** for its failures, grouped by each way it counts, and the rules it counts by.

## One case, from start to finish

Here is the whole idea in one example. This is the input file of case
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
anything outside. Nothing here says how many lines of code or comment the file has, because that
depends on which counter is asking.

tokei's rules say a line holding a string counts as code. All three lines are inside the string, so
under tokei's own rules this file is three lines of code and no comments. Here it is in action:

```
$ linejudge explain 2160 --counter tokei

tokei.default on 2160-long_bracket_string_holding_a_comment_symbol
  by its rules    3 lines, 0 blanks, 3 code, 0 comments
  tokei answers   3 lines, 0 blanks, 2 code, 1 comments   ✗ differs
```

**tokei counted the second line as a comment. Its own rules say it is code.** That is the failure,
and no opinion of ours went into it.

tokei can only hand over its totals, so that failure is found by comparing three numbers. Some
counters can say what they made of *each individual line*, and then the report names the line
itself:

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

Line 2 is plain code and mezura called it a comment. A counter is read this way when it prints one
verdict per line, which takes a single JSON document:
[docs/counter-authors.md](docs/counter-authors.md).

## Running it

Install it, download the counters it knows about, and run them all:

```
cargo install --git https://github.com/loc-conformance/linejudge
linejudge fetch
linejudge check
```

`fetch` downloads each counter that is natively supported at exactly the version named in its adapter file, never at whatever
is newest, so two runs on different days measure the same build. Everything it downloads goes where
this program keeps its own files. Nothing is written into your project.

`check` runs every counter it has a binary for, over every case, and prints a cli report. It answers two
separate questions.

The first is whether a counter agrees with its own declared rules. That needs nothing but the
counter and its rules. For a counter nobody here has measured before, it is the only thing we can
honestly say about it.

The second is whether a counter still answers what it answered the last time we measured it. That
one needs our snapshot of the counter, and it is judged only when the binary you are running is the
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

84 cases, each built around one thing counters get wrong: an escaped quote before a comment, a block
comment that nests, a string carrying a comment symbol, a line spliced onto the next with a
backslash, a second language inside the file. **Every counter measured here fails cases its own
tests never caught**: tokei 40, scc 26, and mezura 8, which was written by the person who built this
suite.

Use it as a CI step, or by hand while you are working on your counter. Either way it takes two
files, and both of them go in your own repository.

The first is `.linejudge/counters.toml` in your repository, saying where your binary is so that
nobody has to pass `--bin` again:

```toml
mycounter = "target/release/mycounter"
```

The path is relative to the folder holding `.linejudge`, and the extension can be left off, so one
line works on Windows, Linux and macOS alike. This works for any counter, including the ones already
supported here: a path in this file wins over anything `fetch` downloaded, so you can measure the
build you just made against the released version of everything else.

The second is a list of the cases you already fail, one per line, named exactly as the report names
them:

```
# the tag it does not recognise, and the two raw string forms
6040-script_tag_in_upper_case
3030-raw_string_ending_in_backslash
3040-bare_raw_string_ending_in_backslash
```

Run with that list and your build breaks only when a case fails that the list does not name:

```
linejudge check --counter mycounter --known-failures known-failures.txt
```

If a case on that list starts passing, the run tells you to delete the line. **It does not fail the
build**: fixing something is never a reason for your CI to go red.

That file is yours, and it is the only thing your build depends on. What we recorded about your
counter is our snapshot of it, it goes stale the moment you fix something, and your CI is
deliberately not judged against it.

Your run can also write a badge for your own README:

```
linejudge check --counter mycounter --badge linejudge.svg
```

It shows how many cases the run covered, and no score: your counter is judged by rules you wrote
yourself.

## Adding native support for your counter here

To be measured here and appear on the results page linked at the top, your counter needs three
things: a file saying how it counts, a file saying how to run it and how to read what it prints,
and a way for us to download it.

Your counter itself does not have to change. The command we run can be a wrapper script, in whatever
language your project already uses. And if your counter already prints JSON of a plain shape, we read
that directly with no wrapper at all.

A counter on that page also gets a badge you can use, one per way it counts. That one shows how
the cases went:

<img src="assets/badge.png" width="198" alt="conformance: 76 agree, 1 fail and nobody has reviewed them, 8 fail">

The steps are in [CONTRIBUTING.md](CONTRIBUTING.md) and the output formats are in
[docs/counter-authors.md](docs/counter-authors.md).

## How it works

Every case records where each string and comment in the file begins and ends. That is a fact about
the language, the same no matter who is asking. Every counter records its own rules for sorting a line
into a category. Run those rules over those facts and you get the number that counter should print for
that file. Then the counter is actually run, and the two are compared.

**What a case brings:**

- **`input.<ext>`** the file being counted. It carries no explanation of itself, because that would
  be a comment and comments get counted.
- **`case.toml`** one sentence saying what trips counters up in this file. It shows on the case's
  page.
- **`truth.txt`** where every string and comment begins and ends, taken from the language spec or a
  lexer for the language, and checked by hand. This is the part that belongs to no counter.

Writing that file by hand would mean counting characters in a column, so there is a short way of
describing where the spans are and a command that turns it into the file (`propose-markers`). Both are
in [cases/README.md](cases/README.md).

**What a counter brings:**

- **`dialects/<counter>/`** its own rules: the categories it names, and what goes into each. This is
  what the right answer is worked out from, which is why a counter is only ever measured against its
  own words. A counter that counts more than one way has one file per way, judged separately.
- **`adapters/<counter>.toml`** how to run it and how to read what it prints. An `[acquisition]` block
  says where to download it and at which version, which is what `fetch` reads.

**What we keep:**

- **`recorded/<counter>.toml`** what the counter printed for every case on the day we measured it, at the
  version written at the top. A counter we have never measured needs no such file to be judged.

Cases are grouped in thousands by what they trip up:

```
1000-comments                            where a comment begins and ends, and how two pairs meet
2000-string_forms                        where a raw, verbatim, heredoc or triple quoted form ends
3000-escapes_and_the_closing_quote       a backslash or backtick before the quote that would close it
4000-a_quote_that_opens_nothing          char literals, primed identifiers, a lone apostrophe
5000-line_splices                        a backslash joining two lines, in a string, a comment or code
6000-another_language_inside_the_file    markup, script and style tags, Vue blocks, PHP
7000-literals_with_their_own_delimiters  a regular expression, and whatever else carries its bounds
8000-what_the_line_counts_as             a blank line inside a comment, a line of punctuation only
```

All four are carried inside the binary, so `check` needs nothing on disk. `--corpus` replaces the
set of test files outright, because a corpus only means anything whole. `--dialects`, `--adapters`
and `--recorded` work the other way and are layered on top per counter: a folder holding one
`mycounter.toml` declares your counter and leaves every other counter exactly as it was.

## Licence

The suite is MIT or Apache-2.0, at your option. The cases carry CC0 instead, in `cases/LICENSE`.
They are dedicated to the public domain, so you can copy any of them into your own repository
without asking and without carrying an attribution line with them.
