# For counter authors

This is for whoever maintains a line-of-code counter and wants it measured here. It covers the three
ways a counter can hand us its numbers, and how a Rust project can skip the executable entirely.

None of it asks you to change your counter. The command we run does not have to be your binary: it can
be a wrapper script in whatever language your project already uses, so no flag is ever added to a
counter for our sake. A project that builds binaries writes the wrapper as a second binary in its own
workspace. A project in an interpreted language already has its interpreter in its own CI.

## If your counter already prints plain JSON

Then no wrapper is needed. A `read` block in your adapter says where each number sits in the
document your counter already produces. Paths are dot-separated names, `[]` reads every element of a
list, and a block naming `each` reads its paths relative to every matched element and adds them up:

```toml
[dialect.default.read]
each     = "[]"
lines    = "Lines"
code     = "Code"
comments = "Comment"
blanks   = "Blank"
```

A block whose paths are absolute reads the document as it stands. `claims` names a path that has to
match at least one element, or the file counts as unclaimed. A `regions` block under it reads the
stretches of another language the same way, with `language` naming where the name of that language
sits.

scc and mezura are both read this way. tokei is not: its document is a map with a key to exclude, it
nests without bound, and it prints no line count at all. A shape like that wants a wrapper rather
than three more path features, so tokei is read by a reader written here on purpose.

## If it does not: the uniform format

Your adapter names whatever command or script you like, in any language, as long as what reaches
stdout is this:

```json
{
  "lines": 13,
  "buckets": { "code": 9, "comments": 3, "blanks": 1 },
  "regions": [
    { "language": "CSS", "lines": 2, "buckets": { "code": 1, "comments": 1, "blanks": 0 } }
  ]
}
```

Then put `output = "linejudge-json"` in the adapter file.

The names inside `buckets` are the ones your own declaration uses, whatever they are. `regions`
names the parts of the file written in another language and is left out when there are none. A file
your counter does not claim is answered with `null`, not with zeroes. Refusing a file is an answer
of its own, and it is not the same as counting nothing in it.

How you produce those numbers is your business, and it stays in your hands.

## Being read line by line

This one is optional and buys you better failure reports. A counter that can say what it made of
each individual line prints one verdict per physical line:

```json
{
  "format": 1,
  "lines": 3,
  "buckets": { "code": 2, "comments": 1, "extra": 0 },
  "per_line": [
    { "line": 1, "bucket": "code" },
    { "line": 2, "bucket": "comments" },
    { "line": 3, "bucket": "code" }
  ]
}
```

Declare it with `explain-output = "linejudge-per-line"` in the adapter, and `linejudge explain` will
name every line your counter read differently from what your rules ask, instead of only showing the
totals disagreeing.

Four things are checked, and a document that breaks one is refused by name. `per_line` holds exactly
`lines` entries. `line` counts from 1 with none missing. Every `bucket` is one your declaration
names. The totals in `buckets` are the per-line verdicts added up. That last check is what stops an
eighteenth verdict for a seventeen line file from being lined up against the wrong lines.

Fields we do not know about are skipped, so you are free to carry your own detail per line. mezura
ships the class it gave the line, the sentence for a string or comment that opened earlier, and the
byte ranges of the line's code, string and comment stretches.

There is a second way, for a counter that prints text for a person rather than JSON. `explain-args`
names the command your counter already has for reading a file line by line, scc's being `-t`. Then
`explain-keep-from` names a piece of text, and only the lines holding it are shown, each one cut to
start at that text. That is how scc's timing traces and summary stay out of the report. Those lines
are chosen and cut, never parsed. If nothing matches, the whole output is shown and the report says
so.

## For a counter written in Rust: judging in-process

A Rust project can skip the executable altogether and judge itself inside its own tests, with this
repository as a dev-dependency:

```toml
[dev-dependencies]
linejudge = { git = "https://github.com/loc-conformance/linejudge" }
```

The dependency brings the corpus with it. The test asks for the corpus, works out the right answer
for each case from your rules, and holds your own in-process counting against it. No binaries and no
adapters anywhere:

```rust
use linejudge::corpus::Corpus;
use linejudge::deriver::derive_answer;
use linejudge::dialects::Dialects;
use linejudge::shipped::create_the_shipped_dir;
use linejudge::verdict::{Conformance, judge_conformance};

#[test]
fn every_case_is_counted_the_way_our_rules_say() {
    let carried = create_the_shipped_dir().unwrap();
    let dialects = Dialects::read(&[carried.join("dialects")]).unwrap();
    let corpus = Corpus::read(&carried.join("cases")).unwrap();
    let rules = dialects.find("tokei", "default").unwrap();
    for case in &corpus.cases {
        let real = derive_answer(&case.truth, rules, &corpus.readings).unwrap().real;
        let counted = count_the_way_the_library_does(&case.input_file);
        assert_eq!(judge_conformance(&real, Some(&counted)), Conformance::Agrees, "{}", case.name);
    }
}
```

`count_the_way_the_library_does` is your own code handing back an `Answer`, which is the same
mapping your adapter would otherwise declare. A counter we do not ship a declaration for reads its own
file instead of `tokei`'s. The cases you deliberately fail are filtered by `case.name`, using the
same names a known-failures file carries.

That `Answer` carries `regions` as well as `counts`, and both are compared. A counter that does not
split a page into its embedded languages leaves `regions` empty and then fails every case holding
one, with all of its counts right. If that is what your counter does, compare `counts` alone.

A dependency declaration never delivers the executable: cargo builds a dependency's library and
leaves its binaries alone. If you want `linejudge check` beside the in-process tests, install it
separately with `cargo install`.
