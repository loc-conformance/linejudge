# Writing a case's truth

What the groups mean, how a case is numbered and what its three files are is covered in the
[README](../README.md) and in [CONTRIBUTING.md](../CONTRIBUTING.md). This page documents one of
the three files: `truth.txt`, the hand-verified record of which bytes of the input are string and
which are comment.

## The marker language

A `truth.txt` holds a copy of every line of the input, and under each copy a marker line with one
character per byte of the line above:

- `S` marks the bytes of a string's opening symbol, `s` the bytes inside it, `Z` the bytes of its
  closing symbol.
- `C`, `c` and `U` say the same about a comment.
- `.` is a byte that belongs to no span, and whitespace outside every span stays whitespace.

From [1070-block_comment_closed_then_line_comment](1000-comments/1070-block_comment_closed_then_line_comment/truth.txt):

```
/* block */ // trailing
CCcccccccUU CCccccccccc
int x = 1;
... . . ..
```

A tag that opens a stretch of another language gets `>` under each of its bytes, with the language
written after the markers, and the tag that closes the stretch gets `<`. Which lines belong to the
stretch is computed from the tags. A stretch that can fairly be counted either way also names its
reading, in parentheses beside the language. From
[6090-vue_blocks_count_as_their_own_languages](6000-another_language_inside_the_file/6090-vue_blocks_count_as_their_own_languages/truth.txt):

```
<template>
>>>>>>>>>> HTML (optional vue-template)
  <!-- a template comment -->
  CCCCccccccccccccccccccccUUU
</template>
<<<<<<<<<<<
```

The copies are there so the file can be checked automatically: `cargo test` refuses a copy that
differs from the input, and a marker line whose length differs from the line above it. So if you
edit an input and forget its truth, the tests fail instead of the markers quietly pointing at the
wrong bytes.

## Writing it with propose-markers, the recommended way

You can write a `truth.txt` by hand, but lining up the marker columns by hand is easy to get
wrong, and that is what this tool takes off you. Describe where the spans and the tags are in a
short spec file, then run, from the root of the repository:

```
cargo run --features maintenance --bin propose-markers -- <spec>
```

The spec notation is documented in the tool's own help, reproduced here word for word (a test
keeps this page and the help identical):

```
propose-markers <spec>

    Run from the root of this repository, it writes the truth.txt of every case the spec names.
    A truth.txt carries one marker line under a copy of each source line, saying of every byte
    whether it is inside a string, inside a comment, part of a tag that bounds another language,
    or none of those. You say where the spans and the tags are; this only fills in the columns,
    so read what it wrote before believing it. 'cargo test' then refuses a truth that does not
    match its input.

    Case 1070 is a worked example. Its first line is

        /* block */ // trailing

    which holds two comments, so its spec is

        == 1070
        1 C /*| block |*/
        1 C //| trailing

    In case 1070, on line 1, there is a comment that the symbol /* opens and the symbol */
    closes, and another that // opens and nothing closes, so it runs to the end of the line.
    The first bar ends the opening symbol, the second begins the closing one; with no second
    bar the span runs to the end of the line, and a closing bar at the very end, like \|"|,
    bounds the span without a closing symbol. What comes out is

        /* block */ // trailing
        CCcccccccUU CCccccccccc

    where C and U mark the bytes of a comment's opening and closing symbols and c the bytes
    between them, S Z and s say the same about a string, a dot is a byte that belongs to no
    span, and a space stays a space.

    A span that crosses lines is one row naming its whole range, with ' ... ' standing for the
    lines between, which need nothing said about them:

        3-5 C /*| ... |*/

    A tag that opens a stretch of another language is a row of kind >, the language in
    parentheses, and the tag that closes it a row of kind <; the same parentheses on an S or C
    row label the span itself as the region, a doc comment read as Markdown being the one in the
    corpus:

        2 > (JavaScript) <SCRIPT>
        5 < </SCRIPT>
        1 C (Markdown optional rust-doc-comment) ///| A documented function.

    A region that is fair to count either way says so with the word optional and the name of the
    reading a counter answers, `rust-doc-comment` rather than `Markdown`, since one counter can
    read a Rust doc comment as Markdown and not a Java one.

    A tag written over two lines is two > rows, and each of them names the language: the same
    language twice is one tag opening one region, and two different ones are two regions nested.
    A row that leaves it out is refused when the truth is read.

        1 > (JavaScript) <script
        2 S "|text/javascript|"
        2 > (JavaScript) >

    Tags nest, and every line belongs to the innermost of them, so a php file whose page holds a
    script is written as two pairs one inside the other:

        2 > (HTML) ?>
        4 > (JavaScript) <script>
        7 < </script>
        9 < <?php

    Lines are numbered from 1 and a line no row names comes out as all dots, which is what a
    line of plain code is. Rows touching one line are written in the order they sit on it, and
    each is searched for after the previous one ends. A text that is still ambiguous is refused,
    and the refusal is answered by appending @2 for the second occurrence.
```

## Check it yourself, always

The tool does not verify the spec: if the spec marks the wrong bytes, the truth comes out wrong,
and `cargo test` cannot tell, because it checks the file against the input, not against the
language. So read the markers yourself before committing. If the language has a lexer that can
print its tokens (`python -m tokenize`, say), run it over the input and compare: the token
positions say where the strings and comments really are.
