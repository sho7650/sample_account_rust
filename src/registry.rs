//! Static field registry. One `Field` per CSV column.
//!
//! Adding a new column = (1) write a new `emit_*` free fn below,
//! (2) add a `Field { … }` entry to `FIELDS`. The CLI parser, --help,
//! and short-option string all derive from this slice.
//!
//! Emit functions append directly to a `Vec<u8>` (matches Go's
//! `strconv.Append*` idiom). This avoids per-row String allocations
//! that dominate cost on large generations.

use std::io::Write as _;

use crate::field::{Deps, RowContext};

pub struct Field {
    pub short: char,
    pub long: &'static str,
    pub desc: &'static str,
    pub emit: fn(&mut Vec<u8>, &RowContext, &mut Deps),
}

// -----------------------------------------------------------------------------
// Identity
// -----------------------------------------------------------------------------

fn emit_id(out: &mut Vec<u8>, ctx: &RowContext, _: &mut Deps) {
    write!(out, "{}", ctx.row + 1).unwrap();
}

// -----------------------------------------------------------------------------
// Person
// -----------------------------------------------------------------------------

fn emit_lastname(out: &mut Vec<u8>, ctx: &RowContext, d: &mut Deps) {
    d.person.append_last_name(out, ctx.last);
}

fn emit_firstname(out: &mut Vec<u8>, ctx: &RowContext, d: &mut Deps) {
    d.person.append_first_name(out, ctx.first);
}

fn emit_mail(out: &mut Vec<u8>, ctx: &RowContext, d: &mut Deps) {
    d.person.append_mail_address(out, ctx.first, ctx.last);
}

fn emit_telephone(out: &mut Vec<u8>, _ctx: &RowContext, d: &mut Deps) {
    let a = d.rng.next_i32().rem_euclid(1000);
    let b = d.rng.next_i32().rem_euclid(1000);
    write!(out, "090-{:04}-{:04}", a, b).unwrap();
}

fn emit_gender(out: &mut Vec<u8>, ctx: &RowContext, d: &mut Deps) {
    out.extend_from_slice(d.person.gender(ctx.first).as_bytes());
}

fn emit_blood(out: &mut Vec<u8>, _ctx: &RowContext, d: &mut Deps) {
    let n = d.rng.next_i32();
    out.extend_from_slice(d.person.blood_type(n).as_bytes());
}

// -----------------------------------------------------------------------------
// Address
// -----------------------------------------------------------------------------

fn emit_prefecture(out: &mut Vec<u8>, ctx: &RowContext, d: &mut Deps) {
    out.extend_from_slice(d.address.prefecture_name(ctx.pref).as_bytes());
}

fn emit_ward(out: &mut Vec<u8>, ctx: &RowContext, d: &mut Deps) {
    out.extend_from_slice(d.address.ward(ctx.pref, ctx.ward).as_bytes());
}

fn emit_city(out: &mut Vec<u8>, ctx: &RowContext, d: &mut Deps) {
    out.extend_from_slice(d.address.city(ctx.pref, ctx.city).as_bytes());
}

// -----------------------------------------------------------------------------
// Age / date / numeric
// -----------------------------------------------------------------------------

fn emit_age(out: &mut Vec<u8>, ctx: &RowContext, d: &mut Deps) {
    write!(out, "{}", d.age_date.age(ctx.age)).unwrap();
}

fn emit_agegroup(out: &mut Vec<u8>, ctx: &RowContext, d: &mut Deps) {
    write!(out, "{}", d.age_date.age_group(ctx.age)).unwrap();
}

fn emit_birthyear(out: &mut Vec<u8>, ctx: &RowContext, d: &mut Deps) {
    write!(out, "{}", d.age_date.birth_year(ctx.age)).unwrap();
}

fn emit_reward(out: &mut Vec<u8>, ctx: &RowContext, d: &mut Deps) {
    let v = d.age_date.reward(ctx.age, d.rng);
    write!(out, "{}", v).unwrap();
}

fn emit_date(out: &mut Vec<u8>, _ctx: &RowContext, d: &mut Deps) {
    write!(out, "{}/{}/{}", d.rng.year(), d.rng.month(), d.rng.day()).unwrap();
}

fn emit_random(out: &mut Vec<u8>, _ctx: &RowContext, d: &mut Deps) {
    let v = (d.rng.next_i32().rem_euclid(20001) - 10000) * 1000;
    write!(out, "{}", v).unwrap();
}

fn emit_quotient(out: &mut Vec<u8>, _ctx: &RowContext, d: &mut Deps) {
    let v = d.rng.next_i32().rem_euclid(100) as f64 / 100.0;
    write!(out, "{:.2}", v).unwrap();
}

// -----------------------------------------------------------------------------
// Registry
// -----------------------------------------------------------------------------

/// Order matches C++ `buildDefaultRegistry` and determines the order columns
/// appear in --help output. The CLI parser respects flag-occurrence order
/// for output column order, NOT this slice's order.
pub const FIELDS: &[Field] = &[
    Field {
        short: 'i',
        long: "id",
        desc: "sequential row id (1-based)",
        emit: emit_id,
    },
    Field {
        short: 'l',
        long: "lastname",
        desc: "last name (kanji,kana — two CSV fields)",
        emit: emit_lastname,
    },
    Field {
        short: 'f',
        long: "firstname",
        desc: "first name (kanji,kana — two CSV fields)",
        emit: emit_firstname,
    },
    Field {
        short: 'm',
        long: "mail",
        desc: "email address (firstname_lastname@example.com)",
        emit: emit_mail,
    },
    Field {
        short: 't',
        long: "telephone",
        desc: "phone number (090-XXXX-XXXX)",
        emit: emit_telephone,
    },
    Field {
        short: 'p',
        long: "prefecture",
        desc: "prefecture name (population-weighted)",
        emit: emit_prefecture,
    },
    Field {
        short: 'w',
        long: "ward",
        desc: "ward / municipality within the prefecture",
        emit: emit_ward,
    },
    Field {
        short: 'c',
        long: "city",
        desc: "city / district within the ward",
        emit: emit_city,
    },
    Field {
        short: 'g',
        long: "gender",
        desc: "gender (男 / 女)",
        emit: emit_gender,
    },
    Field {
        short: 'b',
        long: "blood",
        desc: "ABO blood type",
        emit: emit_blood,
    },
    Field {
        short: 'a',
        long: "age",
        desc: "age in years (population-weighted)",
        emit: emit_age,
    },
    Field {
        short: 'o',
        long: "agegroup",
        desc: "age group rounded down to the decade",
        emit: emit_agegroup,
    },
    Field {
        short: 'y',
        long: "birthyear",
        desc: "birth year derived from age",
        emit: emit_birthyear,
    },
    Field {
        short: 'r',
        long: "reward",
        desc: "annual income-like figure derived from age group",
        emit: emit_reward,
    },
    Field {
        short: 'd',
        long: "date",
        desc: "random valid date (YYYY/M/D)",
        emit: emit_date,
    },
    Field {
        short: 'n',
        long: "random",
        desc: "random signed integer in ±10,000,000",
        emit: emit_random,
    },
    Field {
        short: 'q',
        long: "quotient",
        desc: "random fraction in [0.00, 0.99]",
        emit: emit_quotient,
    },
];

pub fn find_by_short(c: char) -> Option<&'static Field> {
    FIELDS.iter().find(|f| f.short == c)
}

pub fn find_by_long(name: &str) -> Option<&'static Field> {
    FIELDS.iter().find(|f| f.long == name)
}

pub fn short_optstring() -> String {
    FIELDS.iter().map(|f| f.short).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_17_fields() {
        assert_eq!(FIELDS.len(), 17);
    }

    #[test]
    fn short_flags_are_unique() {
        let mut chars: Vec<char> = FIELDS.iter().map(|f| f.short).collect();
        chars.sort();
        let len = chars.len();
        chars.dedup();
        assert_eq!(chars.len(), len, "short flags must be unique");
    }

    #[test]
    fn long_names_are_unique() {
        let mut names: Vec<&str> = FIELDS.iter().map(|f| f.long).collect();
        names.sort();
        let len = names.len();
        names.dedup();
        assert_eq!(names.len(), len, "long names must be unique");
    }

    #[test]
    fn find_by_short_works() {
        assert_eq!(find_by_short('i').unwrap().long, "id");
        assert_eq!(find_by_short('m').unwrap().long, "mail");
        assert!(find_by_short('z').is_none());
    }

    #[test]
    fn find_by_long_works() {
        assert_eq!(find_by_long("telephone").unwrap().short, 't');
        assert!(find_by_long("nonexistent").is_none());
    }

    #[test]
    fn short_optstring_concatenates_short_flags() {
        let s = short_optstring();
        assert_eq!(s.len(), FIELDS.len());
        assert!(s.contains('i'));
        assert!(s.contains('q'));
    }
}
