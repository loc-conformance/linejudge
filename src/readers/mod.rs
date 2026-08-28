//! One file for every counter whose output no declarative `read` block in the adapter can
//! describe, and which has no other way of handing linejudge its data in the format it expects.
//!
//! Here, counter-specific reader functions are written, turning the data from the format the
//! counter itself provides into the format linejudge recognizes. A reader exists for whichever
//! output needs it: the counts, or the per-line account `explain` shows. The function's name
//! says which one it reads, `read_counts` or `read_per_line`, and a counter that some day needs
//! both holds both, in its one file.
//!
//! It is a deliberate choice for these readers to be written and compiled as part of this
//! codebase, rather than using a more open convention, like running a separate script that does
//! the transformation, for two reasons. The first is that they run natively as part of the
//! application, rather than requiring the local environment to have all the specific tools and
//! interpreters to run the provided scripts. The second is that getting the output of a counter
//! accurately is one of the most important parts of linejudge, and this way it can more easily
//! be verified and tested.
pub mod cloc;
pub mod scc;
pub mod tokei;
