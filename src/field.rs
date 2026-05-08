//! Per-row context and dependency bundle passed to each Field's emit fn.
//!
//! Mirrors C++ `field.h` — `RowContext` holds the random integers drawn
//! once per row so multiple fields can read the same value (e.g. `first`
//! drives both first-name and email).

use crate::generators::{AddressGenerator, AgeAndDateGenerator, PersonGenerator};
use crate::rng::Rng;

#[derive(Clone, Copy, Debug)]
pub struct RowContext {
    pub row: i32,
    pub first: i32,
    pub last: i32,
    pub pref: usize,
    pub ward: i32,
    pub city: i32,
    pub age: i32,
}

pub struct Deps<'a, 'p, 'pr, 'ag> {
    pub person: &'a PersonGenerator<'p>,
    pub address: &'a AddressGenerator<'pr>,
    pub age_date: &'a AgeAndDateGenerator<'ag>,
    pub rng: &'a mut Rng,
}
