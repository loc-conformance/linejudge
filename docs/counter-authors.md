# For counter authors

This is for whoever maintains a line-of-code counter and wants it measured: privately from its
own repository, or supported natively here with a column on the public webpage.
It covers the files a counter brings, the ways it hands over its numbers, what measuring privately needs,
how to add native support, and how a Rust project can skip the linejudge executable entirely.

**No counter is ever required to add a new flag to use linejudge.** There are different ways for a counter
to hand over its numbers to linejudge, with the intention of the counter program not needing any change
in its code to do so.

- [What a measured counter consists of](#what-a-measured-counter-consists-of)
- [Handing over the numbers](#handing-over-the-numbers)
- [Measuring from your own repository](#measuring-from-your-own-repository)
- [If your counter already has native support](#if-your-counter-already-has-native-support)
- [Getting native support here](#getting-native-support-here)
- [Being read line by line](#being-read-line-by-line)
- [For a counter written in Rust: judging in-process](#for-a-counter-written-in-rust-judging-in-process)

## What a measured counter consists of

Three things privately, four with native support.   
A **dialect**, `dialects/<counter>/`, holds the
counter's own rules: the categories it names, and what goes into each, one file per way it counts;
the format has its own page in [dialects/README.md](../dialects/README.md).  
An **adapter**, `adapters/<counter>.toml`, says how to run the binary and how to read what it prints,
and most of this page is about that.  
And the **binary** itself. It can be named by a path or downloaded by the `fetch`
command, if the counter has native support in the linejudge repo.  

Beside them, a natively supported counter also keeps `recorded/<counter>.toml`: our snapshot of what
the counter answered on the day it was measured. A counter measured privately doesn't need it.

## Handing over the numbers

The adapter reads the counter's answer in one of **three ways**.

### 1) A `read` block over your own JSON

If your counter already prints plain JSON, no code is needed anywhere. A `read` block in your
adapter says where each number sits in the document your counter already produces. Paths are
dot-separated names, `[]` reads every element of a list, and a block naming `each` reads its paths
relative to every matched element and adds them up:

```toml
[dialect.<name>.read]
each     = "[]"
lines    = "Lines"
code     = "Code"
comments = "Comment"
blanks   = "Blank"
```

A block whose paths are absolute reads the document as it stands. `claims` names a path that has
to match something, one value or at least one element of a list, or the file counts as unclaimed.
A `regions` block under it reads the stretches of another language the same way, with `language`
naming where the name of that language sits. mezura's `content` dialect shows all three at once,
`extra` being a category of mezura's own naming:

```toml
[dialect.<name>.read]
claims   = "languages[]"
lines    = "total.lines"
code     = "total.code"
comments = "total.comments"
extra    = "total.extra"

[dialect.<name>.read.regions]
each     = "languages[].nested_languages[]"
language = "name"
lines    = "lines"
code     = "code"
comments = "comments"
extra    = "extra"
```

The claim is met by any element under `languages`, the totals are read off the document's own
`total` object, and every nested language found by `each` becomes one region, named by its `name`
field.

cloc, scc and mezura are all read this way. tokei is not: its document is a map with a key to
exclude, it nests without bound, and it prints no line count at all. A shape like that is read by
the third way below.

### 2) The uniform format

The second way is to provide directly the data linejudge reads, from your counter:

```json
{
  "format": 1,
  "lines": 13,
  "buckets": { "code": 9, "comments": 3, "blanks": 1 },
  "regions": [
    { "language": "CSS", "lines": 2, "buckets": { "code": 1, "comments": 1, "blanks": 0 } }
  ]
}
```

Then put `output = "linejudge-json"` in the `adapter` file. `format` names the version of this
document, and what this page describes is format 1.

The names inside `buckets` are the ones your own declaration uses, whatever they are. `regions`
names the parts of the file written in another language and is left out when there are none. A file
your counter does not claim is answered with `null`, not with zeroes. Refusing a file is an answer
of its own, and it is not the same as counting nothing in it.

How you produce those numbers is your business, and it stays in your hands.

### 3) A `reader` compiled here

The third way is for a document that the `read` block cannot describe, or from one that doesn't
support JSON. The `adapter` names the format outright and a reader compiled into this
repository does the reading: tokei is read this way, and its `adapter` says `output = "tokei-json"`.
Those readers live one file per counter, under `src/readers/`.

Reach for this last: a `read` block is a few lines of TOML and the uniform format is yours to
print, while this is code in somebody else's repository. And it is deliberate that it is compiled
code rather than a script. What is read here decides verdicts, and `linejudge check` has to run on
a machine holding nothing but the fetched binaries, so nothing on that path is allowed to need an
interpreter or dependency that may not be installed.  
While a script separate from the codebase cannot be used to read the output format, it can be used
to read the by-line explanations, if a counter provides such functionality
(see [Being read line by line](#being-read-line-by-line)).

## Measuring from your own repository

Use the corpus as a CI step, as a test inside your own test suite, or by hand while you work on
your counter. `linejudge check` exits non-zero when a case fails that your `--known-failures` list
does not name, so any test suite that can run a program and read an exit code can include it as a
test, whatever language the suite is written in. Everything below lives in your repository.

This whole section is for a counter this binary knows nothing about. For one already supported
here, see [If your counter already has native support](#if-your-counter-already-has-native-support).

The declaration travels with your repository in a `.linejudge` folder, which linejudge finds by
walking up from wherever it runs. What sits inside it under a fixed name is taken with no flag and
no declaration: an `adapters`, `dialects`, `explain-scripts` or `recorded` folder in there is layered
over what the binary carries, and a `known-failures` folder holds one `<counter>.txt` per counter.
A `cases` folder is taken too, but it replaces the built-in corpus rather than adding to it. The
one file always needed is `counters.toml`, naming the binary so nobody has to pass `--bin` again:

```toml
mycounter = "target/release/mycounter"
```

So the whole folder of a typical counter is:

```
.linejudge/
  counters.toml
  adapters/mycounter.toml
  dialects/mycounter/default.toml
  known-failures/mycounter.txt
```

The `known-failures` file is the gate: one case per line, named exactly as the report names them,
and a `#` line for a comment:

```
# the tag it does not recognise, and the two raw string forms
6040-script_tag_in_upper_case
3030-raw_string_ending_in_backslash
3040-bare_raw_string_ending_in_backslash
```

If a case on the list starts passing, the run tells you to delete the line, and it does not fail
the build. The run can also write a badge for your repository, `--badge linejudge.svg`, showing
the version it ran as and how many cases it covered. Pin the LineJudge version your CI installs,
and when you raise it, update the list in the same change.

A declaration kept elsewhere is named in a `settings.toml` beside `counters.toml`, whose keys are
`adapters`, `dialects`, `explain-scripts`, `recorded` and `corpus`; a relative path there resolves against
the folder holding `.linejudge`, and a named path beats the fixed name. A flag beats everything in
the folder, and the folder beats the defaults.

The adapter a counter starts with is small:

```toml
name   = "mycounter"
output = "linejudge-json"
args   = ["{file}"]

[dialect.default]
args = []
```

`args` is the command line, with `{file}` standing for the case being counted, and one
`[dialect.<name>]` block exists per way the counter counts, each adding its own arguments. The
rules themselves go in `dialects/mycounter/default.toml`, as the tree above shows. `output` here
names the second of the three ways above; a `read` block takes its place when the first fits. A `version-flag` line is
optional, and without one the version reads as unknown, which costs nothing privately, since a
counter measured privately has no recorded snapshot to be judged against.

Of the three ways of handing over the numbers, the first two work unchanged here. The third
cannot: a reader is code compiled into linejudge itself, and nothing in your repository can add to
that. When neither of the two fits, the way out is not a linejudge feature at all. Point
`counters.toml`, or `--bin`, at a program of your own that runs the real
counter and prints the uniform format. The adapter still says `output = "linejudge-json"`, and no
field, flag or line of code anywhere knows the difference: linejudge measures the thing it was
pointed at, and what that thing is made of is yours. Write it in whatever language your project
already uses, a second binary of the workspace for a compiled project, a script beside the counter
for an interpreted one: the machine it runs on is yours and already carries that runtime, which is
exactly what the fetched path may not assume. The same program serves the explain side: declare
`explain-args` beside `explain-output = "linejudge-per-line"`, and the arguments you chose reach
your program, which prints the per-line document described further down.

## If your counter already has native support

Its `adapter` and dialects ship inside the linejudge binary, so almost nothing is declared. All
your repository needs is a `.linejudge` folder holding a `counters.toml` pointing to your binary,
which wins over the fetched release, and a `known-failures/<counter>.txt`: that can be updated
along with your bugfixes. Or skip the folder and hand the same things over as flags: `--bin <path>`
names your binary, and `--known-failures <file>` your list.

To try a deliberate change to your counting rules before it is merged here, put a copy of your
dialect file with the changed rule under `.linejudge/dialects/<counter>/`: a folder there wins
over the shipped copy for your counter alone, so the run judges your build against the rules you
are about to propose.

## Getting native support here

To be merged, a counter needs the dialect folder, the adapter, and one thing more inside the
adapter: an `[acquisition]` block naming where the binary is downloaded from and at which version.
Without it nobody here can download the counter: we could not check what we are merging, could not
re-measure it when it releases, and the automated runs that build the results page would leave it
out. If your counter is distributed in a way the existing channels do not cover, open an issue
about adding a channel instead of leaving the block out.

Measuring from your own repository also works before merging: measure privately for as long as
you want, and the pull request that adds the files just makes it public. A counter that cannot be
downloaded at all still gets everything except the results page, from its own side.

A counter that never prints our per-line document can still be read line by line here: cloc and
scc both are, each by a reader compiled under `src/readers/`, as
[Being read line by line](#being-read-line-by-line) describes.

Once it is merged, the counter's answers are recorded: `recorded/<counter>.toml` holds what it
printed for every case at the version written at the top, and every failure in it carries a note
saying what the counter did, written from runs of the counter itself. The commands for that live
in [CONTRIBUTING.md](../CONTRIBUTING.md#re-measuring-a-counter), under *Re-measuring a counter*.
A counter on the results
page also gets a badge per way it counts, showing how the cases went.

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

Declare the command that prints it in `explain-args` and its shape in
`explain-output = "linejudge-per-line"`, and `linejudge explain` will name every line your counter
read differently from what your rules ask, instead of only showing the totals disagreeing. The two
fields come together or not at all: a per-line command whose output nothing could read is refused
when the adapter is read.

`format` means here what it means in the counts document: which version of the document this is,
and this page describes format 1.
Four more things are checked, and a document that breaks one is refused by name. `per_line` holds exactly
`lines` entries. `line` counts from 1 with none missing. Every `bucket` is one your declaration
names. The totals in `buckets` are the per-line verdicts added up. That last check is what stops an
eighteenth verdict for a seventeen line file from being lined up against the wrong lines.

Fields we do not know about are skipped, so you are free to carry your own detail per line. mezura
ships the class it gave the line, the sentence for a string or comment that opened earlier, and the
byte ranges of the line's code, string and comment stretches.

There is a second way, the mirror of the reader on the counting side, for a counter whose
per-line answer exists but comes out in a shape of its own. A reader compiled under `src/readers/`
turns what the counter prints into the same document, and the adapter names the format in
`explain-output`. Both of ours are read this way. scc's `-t` prints one
`line N ended with state: S: counted as code` per line beside its counts, and its adapter says
`explain-output = "scc-trace"`. cloc says nothing about where it put each line at all, and its
reader works the answer out from `--print-filter-stages`, which prints what is left of the file
after each of its comment filters. Its adapter says `explain-output = "cloc-stages"`.

**What cloc's reader produces is worked out from the outside, and it is not the counter's own
word.** mezura says where it put each line, and cloc does not, so where its lines cannot be told
apart, because cloc joined two of them into one, the reader refuses, and the report says what
could not be read. A reader that cannot honestly answer is expected to do exactly that. Whichever
way a per-line answer arrives, it lands in the same document and the checks above refuse it by
the same rules.

There is a third way, for a counter measured from its own repository whose per-line answer needs
working out by a program of yours. The stand-in program already covers it, as
[Measuring from your own repository](#measuring-from-your-own-repository) says: the explain
arguments reach whatever `counters.toml` names. For the case where the binary is your real
counter and a separate script does the reading, `explain-command` names the program run in place
of your counter, `{binary}` stands for your counter and `{explain-scripts}` for the directory the
script is kept in, `.linejudge/explain-scripts` by its fixed name or `--explain-scripts` by flag:

```toml
explain-command = "python"
explain-args    = ["{explain-scripts}/mycounter.py", "{binary}", "{file}"]
explain-output  = "linejudge-per-line"
```

The script prints the same document as any other per-line answer, and it needs whatever runs it,
python here, to be on the machine. When that is missing the report says so and the rest of the
run is unaffected: nothing that decides a verdict is ever a script.

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
