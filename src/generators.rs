//! Domain-level generators that wrap repositories with lookup logic.
//!
//! Mirrors C++ `generators.h`/`generators.cpp`. Holds borrowed slices /
//! refs into repositories; repositories own the data.

use crate::repos::{AgeBucket, AgeRepo, PersonRecord, PrefectureRepo};
use crate::rng::{current_year, Rng};

const MAIL_DOMAIN: &str = "example.com";

/// Pre-computes the cumulative population running total per prefecture so
/// `weighted_prefecture_index` and `address_index` can do O(1)/O(log n)
/// lookups instead of O(n) per row. Built once at AddressGenerator
/// construction.
struct AddressIndex {
    /// Cumulative `prefectures[i].population` for i in 0..n. Last entry =
    /// total_population.
    pop_cum: Vec<i32>,
    /// Cumulative `prefectures[i].zips` for i in 0..n. `addr_offsets[i]` =
    /// starting index into addresses for prefecture i. Last entry = total
    /// addresses.
    addr_offsets: Vec<i32>,
}

// -----------------------------------------------------------------------------
// PersonGenerator
// -----------------------------------------------------------------------------

pub struct PersonGenerator<'a> {
    records: &'a [PersonRecord],
}

impl<'a> PersonGenerator<'a> {
    pub fn new(records: &'a [PersonRecord]) -> Self {
        assert!(!records.is_empty(), "person records must not be empty");
        Self { records }
    }

    fn pick(&self, n: i32) -> &PersonRecord {
        let len = self.records.len();
        let idx = (n.rem_euclid(len as i32)) as usize;
        &self.records[idx]
    }

    /// Appends "<kanji>,<kana>" — TWO comma-separated CSV fields — to
    /// `out`. Allocation-free on the hot path (caller's buffer grows in
    /// place via `extend_from_slice`).
    pub fn append_last_name(&self, out: &mut Vec<u8>, n: i32) {
        let r = self.pick(n);
        out.extend_from_slice(r.last_kanji.as_bytes());
        out.push(b',');
        out.extend_from_slice(r.last_kana.as_bytes());
    }

    pub fn append_first_name(&self, out: &mut Vec<u8>, n: i32) {
        let r = self.pick(n);
        out.extend_from_slice(r.first_kanji.as_bytes());
        out.push(b',');
        out.extend_from_slice(r.first_kana.as_bytes());
    }

    /// Appends `<first_romaji>_<last_romaji>@example.com` to `out`.
    pub fn append_mail_address(&self, out: &mut Vec<u8>, first: i32, last: i32) {
        let f = self.pick(first);
        let l = self.pick(last);
        out.extend_from_slice(f.first_name.as_bytes());
        out.push(b'_');
        out.extend_from_slice(l.last_name.as_bytes());
        out.push(b'@');
        out.extend_from_slice(MAIL_DOMAIN.as_bytes());
    }

    pub fn gender(&self, n: i32) -> &str {
        &self.pick(n).gender
    }

    pub fn blood_type(&self, n: i32) -> &str {
        &self.pick(n).blood_type
    }
}

// -----------------------------------------------------------------------------
// AddressGenerator
// -----------------------------------------------------------------------------

pub struct AddressGenerator<'a> {
    repo: &'a PrefectureRepo,
    index: AddressIndex,
}

impl<'a> AddressGenerator<'a> {
    pub fn new(repo: &'a PrefectureRepo) -> Self {
        assert!(
            !repo.prefectures.is_empty(),
            "prefecture repo must not be empty"
        );
        // Pre-compute prefix sums so per-row weighted-index and address-
        // index lookups don't iterate the whole prefecture list.
        let n = repo.prefectures.len();
        let mut pop_cum = Vec::with_capacity(n);
        let mut addr_offsets = Vec::with_capacity(n + 1);
        let mut pop_running: i32 = 0;
        let mut addr_running: i32 = 0;
        addr_offsets.push(0);
        for p in &repo.prefectures {
            pop_running = pop_running.saturating_add(p.population);
            pop_cum.push(pop_running);
            addr_running = addr_running.saturating_add(p.zips);
            addr_offsets.push(addr_running);
        }
        Self {
            repo,
            index: AddressIndex {
                pop_cum,
                addr_offsets,
            },
        }
    }

    /// Picks a prefecture index weighted by population. O(log n) via
    /// binary search on the pre-computed cumulative population.
    pub fn weighted_prefecture_index(&self, n: i32) -> usize {
        let total = self.repo.total_population.max(1);
        let target = n.rem_euclid(total);
        // partition_point: returns the first index where pop_cum[i] > target.
        // Equivalent to the original linear "running > target" check.
        self.index
            .pop_cum
            .partition_point(|&cum| cum <= target)
            .min(self.repo.prefectures.len() - 1)
    }

    pub fn prefecture_name(&self, idx: usize) -> &str {
        let safe = if idx < self.repo.prefectures.len() {
            idx
        } else {
            0
        };
        &self.repo.prefectures[safe].name
    }

    fn address_index(&self, pref_idx: usize, n: i32) -> usize {
        let prefs = &self.repo.prefectures;
        let total_addr = self.repo.addresses.len();
        let pref_idx = if pref_idx < prefs.len() { pref_idx } else { 0 };

        let offset = self.index.addr_offsets[pref_idx];
        let zips = prefs[pref_idx].zips;
        if zips <= 0 {
            return (offset as usize).min(total_addr.saturating_sub(1));
        }
        let n = if n < 0 { -n } else { n };
        let idx = (n.rem_euclid(zips)) as usize + offset as usize;
        idx.min(total_addr.saturating_sub(1))
    }

    pub fn ward(&self, pref_idx: usize, n: i32) -> &str {
        &self.repo.addresses[self.address_index(pref_idx, n)].ward
    }

    pub fn city(&self, pref_idx: usize, n: i32) -> &str {
        &self.repo.addresses[self.address_index(pref_idx, n)].city
    }
}

// -----------------------------------------------------------------------------
// AgeAndDateGenerator
// -----------------------------------------------------------------------------

pub struct AgeAndDateGenerator<'a> {
    repo: &'a AgeRepo,
    /// Cached calendar year. `birth_year` reads this instead of calling
    /// `current_year()` (which reads env vars) on every row.
    now_year: i32,
}

impl<'a> AgeAndDateGenerator<'a> {
    /// Construct with an explicit "now year". Used by the runner so the
    /// hot loop never reads env vars.
    pub fn with_year(repo: &'a AgeRepo, now_year: i32) -> Self {
        assert!(!repo.buckets.is_empty(), "age repo must not be empty");
        Self { repo, now_year }
    }

    /// Convenience that captures the current year once at construction.
    pub fn new(repo: &'a AgeRepo) -> Self {
        Self::with_year(repo, current_year())
    }

    fn find_bucket(&self, total: i32) -> &AgeBucket {
        let buckets = &self.repo.buckets;
        let mut i = 0usize;
        while i + 1 < buckets.len() && buckets[i + 1].start <= total {
            i += 1;
        }
        &buckets[i]
    }

    pub fn age(&self, n: i32) -> i32 {
        let total = n.rem_euclid(self.repo.total_age.max(1));
        let bucket = self.find_bucket(total);
        bucket.generation + n.rem_euclid(5)
    }

    pub fn age_group(&self, n: i32) -> i32 {
        let total = n.rem_euclid(self.repo.total_age.max(1));
        let bucket = self.find_bucket(total);
        (bucket.generation / 10) * 10
    }

    pub fn birth_year(&self, n: i32) -> i32 {
        self.now_year - self.age(n)
    }

    pub fn reward(&self, n: i32, rng: &mut Rng) -> i32 {
        let group = self.age_group(n);
        let r1 = rng.next_i32();
        let r2 = rng.next_i32();
        (50 - (group - 50).abs() + (r1.rem_euclid(5))) * (r2.rem_euclid(3) + 1) * 100_000
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repos::{AddressRecord, AgeBucket, PrefectureRecord};

    fn fake_persons() -> Vec<PersonRecord> {
        vec![
            PersonRecord {
                last_kanji: "佐藤".into(),
                last_kana: "サトウ".into(),
                last_name: "sato".into(),
                first_kanji: "太郎".into(),
                first_kana: "タロウ".into(),
                first_name: "taro".into(),
                gender: "男".into(),
                blood_type: "A".into(),
            },
            PersonRecord {
                last_kanji: "鈴木".into(),
                last_kana: "スズキ".into(),
                last_name: "suzuki".into(),
                first_kanji: "花子".into(),
                first_kana: "ハナコ".into(),
                first_name: "hanako".into(),
                gender: "女".into(),
                blood_type: "O".into(),
            },
        ]
    }

    fn fake_pref_repo() -> PrefectureRepo {
        PrefectureRepo {
            prefectures: vec![
                PrefectureRecord {
                    number: 1,
                    name: "Tokyo".into(),
                    population: 100,
                    zips: 2,
                },
                PrefectureRecord {
                    number: 2,
                    name: "Osaka".into(),
                    population: 50,
                    zips: 1,
                },
            ],
            addresses: vec![
                AddressRecord {
                    number: 1,
                    prefecture: "Tokyo".into(),
                    ward: "Shibuya".into(),
                    city: "Ebisu".into(),
                },
                AddressRecord {
                    number: 1,
                    prefecture: "Tokyo".into(),
                    ward: "Shinjuku".into(),
                    city: "".into(),
                },
                AddressRecord {
                    number: 2,
                    prefecture: "Osaka".into(),
                    ward: "Kita".into(),
                    city: "Umeda".into(),
                },
            ],
            total_population: 150,
        }
    }

    fn fake_age_repo() -> AgeRepo {
        AgeRepo {
            buckets: vec![
                AgeBucket {
                    generation: 0,
                    population: 100,
                    start: 0,
                },
                AgeBucket {
                    generation: 30,
                    population: 200,
                    start: 100,
                },
                AgeBucket {
                    generation: 60,
                    population: 50,
                    start: 300,
                },
            ],
            total_age: 350,
        }
    }

    fn appended(f: impl FnOnce(&mut Vec<u8>)) -> String {
        let mut buf = Vec::new();
        f(&mut buf);
        String::from_utf8(buf).expect("invalid UTF-8")
    }

    #[test]
    fn person_last_name_returns_kanji_kana_pair() {
        let recs = fake_persons();
        let g = PersonGenerator::new(&recs);
        assert_eq!(appended(|b| g.append_last_name(b, 0)), "佐藤,サトウ");
        assert_eq!(appended(|b| g.append_last_name(b, 1)), "鈴木,スズキ");
        assert_eq!(appended(|b| g.append_last_name(b, 2)), "佐藤,サトウ"); // wraps
    }

    #[test]
    fn person_mail_address_format() {
        let recs = fake_persons();
        let g = PersonGenerator::new(&recs);
        assert_eq!(
            appended(|b| g.append_mail_address(b, 0, 1)),
            "taro_suzuki@example.com"
        );
    }

    #[test]
    fn weighted_prefecture_picks_first_when_n_zero() {
        let repo = fake_pref_repo();
        let g = AddressGenerator::new(&repo);
        assert_eq!(g.weighted_prefecture_index(0), 0); // Tokyo (target 0 < 100)
    }

    #[test]
    fn weighted_prefecture_picks_second_in_population_band() {
        let repo = fake_pref_repo();
        let g = AddressGenerator::new(&repo);
        // n=120, total=150 → target=120, runs through Tokyo(100) into Osaka.
        assert_eq!(g.weighted_prefecture_index(120), 1);
    }

    #[test]
    fn address_index_respects_per_pref_offset() {
        let repo = fake_pref_repo();
        let g = AddressGenerator::new(&repo);
        // pref 0 has 2 zips → addresses 0..2
        assert_eq!(g.ward(0, 0), "Shibuya");
        assert_eq!(g.ward(0, 1), "Shinjuku");
        assert_eq!(g.ward(0, 2), "Shibuya"); // wraps
                                             // pref 1 has 1 zip starting at offset 2 → address 2
        assert_eq!(g.ward(1, 0), "Kita");
    }

    #[test]
    fn age_picks_first_bucket_for_low_total() {
        let repo = fake_age_repo();
        let g = AgeAndDateGenerator::new(&repo);
        // n=0 → total=0, bucket[0] generation=0, age = 0 + (0 % 5) = 0
        assert_eq!(g.age(0), 0);
        // n=5 → total=5, bucket[0], age = 0 + 0 = 0
        assert_eq!(g.age(5), 0);
        // n=2 → bucket[0], age = 0 + 2 = 2
        assert_eq!(g.age(2), 2);
    }

    #[test]
    fn age_group_rounds_to_decade() {
        let repo = fake_age_repo();
        let g = AgeAndDateGenerator::new(&repo);
        // n=150 → total=150, finds bucket[1] (generation=30) → group=30
        assert_eq!(g.age_group(150), 30);
        // n=320 → total=320, finds bucket[2] (generation=60) → group=60
        assert_eq!(g.age_group(320), 60);
    }
}
