//! Several things wrong at once, which is what reading a corpus or a set of rules finds.

use std::error::Error;
use std::fmt;
use std::ops::Deref;

/// Everything wrong with what was read, in the order it was found. Reading stops at nothing, so a
/// corpus with three broken cases names all three instead of the first.
///
/// It prints one fault per line and is a [`std::error::Error`], so `?` carries it into a
/// `Box<dyn Error>` the way any other error does. It also derefs to a slice, so the faults can be
/// walked, counted and joined as they could when this was a plain vector.
#[derive(Debug)]
pub struct Faults(Vec<String>);

impl Deref for Faults {
    type Target = [String];

    fn deref(&self) -> &[String] {
        &self.0
    }
}

impl fmt::Display for Faults {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0.join("\n"))
    }
}

impl Error for Faults {}

impl From<Vec<String>> for Faults {
    fn from(faults: Vec<String>) -> Faults {
        Faults(faults)
    }
}

impl From<String> for Faults {
    fn from(fault: String) -> Faults {
        Faults(vec![fault])
    }
}
