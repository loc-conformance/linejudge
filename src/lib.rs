#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Holds line-of-code counters to their own declared way of counting.
//!
//! A case is one small input file with every string and comment in it marked by hand, in a
//! `truth.txt` beside it. A dialect is one counter's way of counting, written as rules over those
//! marks: which of them makes a line code, a comment or blank. Reading a case through a dialect
//! gives the answer that counter should print for that file, and that is what its real answer is
//! held against.
//!
//! [`corpus`] and [`dialects`] read the two, [`deriver`] puts them together, [`adapter`] runs a
//! counter's binary, and [`verdict`] compares. [`shipped`] writes out the corpus this build
//! carries, for a project that depends on this crate and has no checkout of it.

pub mod adapter;
pub mod answer;
pub mod corpus;
pub mod deriver;
pub mod dialects;
pub mod faults;
pub mod known_failures;
pub mod per_line;
pub mod readings;
pub mod recorded;
pub mod shipped;
pub mod truth;
pub mod verdict;

pub(crate) mod locator;
pub(crate) mod measurement;
pub(crate) mod readers;
