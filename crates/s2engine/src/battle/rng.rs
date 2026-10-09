//! The original random number generator, reproduced call for call.
//!
//! RNG API (stable):
//! - [`Rng::new(seeder)`]            fresh state as at program start (`holdrand = 1`)
//! - [`Rng::init_table`]             `FUN_0046d6b5`: build the 0..99 roll table
//! - [`Rng::pct`]                    `FUN_0046d776`: table roll 0..=99 ("percent")
//! - [`Rng::rnd(n)`]                 `FUN_0046d7ca`: `rand() % n` (0 for n == 0)
//! - [`Rng::crt_rand`], [`Rng::srand`]  MSVC `rand` / `srand` (`FUN_0047abfd` / `FUN_0047abf3`)
//! - [`Seeder`]                      source of the periodic reseed (`time(NULL)` in the original)
//!
//! How the original seeds: the CRT state starts at 1 (no `srand` at start-up).
//! `FUN_00424450` builds the roll table early in `WinMain` with 1200 `rnd`
//! calls, so the first 199 draws come from seed 1 and then every 200th
//! `pct`/`rnd` call reseeds with `time(NULL)` (wall-clock seconds,
//! `FUN_0047ac1b`) before drawing. A real-time port passes [`Seeder::wall_clock`];
//! tests and replays pass a fixed sequence.

/// `S2_RNG_TRACE=1`: print every `pct` / `rnd` draw to stderr (oracle
/// comparisons: `--watch 46d776,46d7ca` logs the original's calls).
fn rng_tracing() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("S2_RNG_TRACE").is_some())
}

/// With `S2_RNG_TRACE=2`, the battle functions on the call stack.
fn rng_caller() -> String {
    if std::env::var_os("S2_RNG_TRACE").is_some_and(|v| v == "2") {
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        let frames: Vec<&str> = bt
            .lines()
            .filter_map(|l| l.trim().split_once(": ").map(|(_, f)| f))
            .filter(|f| f.contains("battle::") && !f.contains("rng::"))
            .take(4)
            .collect();
        format!("  <- {}", frames.join(" <- "))
    } else {
        String::new()
    }
}

/// Supplies the value of `time(NULL)` at each periodic reseed.
pub struct Seeder(Box<dyn FnMut() -> u32 + Send>);

impl Seeder {
    pub fn new(f: impl FnMut() -> u32 + Send + 'static) -> Seeder {
        Seeder(Box::new(f))
    }

    /// `time(NULL)`: seconds since 1970 (what the original uses).
    pub fn wall_clock() -> Seeder {
        Seeder::new(|| {
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as u32).unwrap_or(0)
        })
    }

    /// Always reseed with `seed` (deterministic replays).
    pub fn fixed(seed: u32) -> Seeder {
        Seeder::new(move || seed)
    }

    /// Reseed with `start`, `start + 1`, ... (a clock that ticks once per reseed).
    pub fn counting(start: u32) -> Seeder {
        let mut t = start;
        Seeder::new(move || {
            let v = t;
            t = t.wrapping_add(1);
            v
        })
    }
}

impl std::fmt::Debug for Seeder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Seeder")
    }
}

#[derive(Debug)]
pub struct Rng {
    /// `DAT_0048fc10`: MSVC `holdrand`.
    state: u32,
    /// `DAT_004a2690`: permutation of 0..=99 used by [`Rng::pct`].
    table: [u8; 100],
    /// `DAT_004a2af8`: draws since the last reseed.
    counter: i32,
    seeder: Seeder,
}

impl Rng {
    /// State at program start. Call [`Rng::init_table`] before using [`Rng::pct`],
    /// as `FUN_00424450` does.
    pub fn new(seeder: Seeder) -> Rng {
        let mut table = [0u8; 100];
        for (i, v) in table.iter_mut().enumerate() {
            *v = i as u8;
        }
        Rng { state: 1, table, counter: 0, seeder }
    }

    /// Program-start state with the roll table already built (the usual setup).
    pub fn started(seeder: Seeder) -> Rng {
        let mut r = Rng::new(seeder);
        r.init_table();
        r
    }

    /// Port of FUN_0047abf3 (`srand`).
    pub fn srand(&mut self, seed: u32) {
        self.state = seed;
    }

    /// Port of FUN_0047abfd (`rand`).
    pub fn crt_rand(&mut self) -> i32 {
        self.state = self.state.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
        ((self.state >> 16) & 0x7fff) as i32
    }

    fn tick(&mut self) {
        self.counter += 1;
        if self.counter > 199 {
            let t = (self.seeder.0)();
            self.srand(t);
            self.counter = 0;
        }
    }

    /// Port of FUN_0046d776: a table roll in 0..=99.
    pub fn pct(&mut self) -> i32 {
        self.tick();
        let r = self.crt_rand();
        let v = self.table[(r % 100) as usize] as i8 as i32;
        if rng_tracing() {
            eprintln!("[rng] pct -> {v}{}", rng_caller());
        }
        v
    }

    /// Port of FUN_0046d7ca: `rand() % n`; `n == 0` returns 0 without a draw.
    pub fn rnd(&mut self, n: i32) -> i32 {
        if n == 0 {
            return 0;
        }
        self.tick();
        // C `%` on (int)rand() and n: rand() >= 0, so the sign follows n's.
        let v = self.crt_rand() % n;
        if rng_tracing() {
            eprintln!("[rng] rnd({n}) -> {v}{}", rng_caller());
        }
        v
    }

    /// Port of FUN_0046d6b5: transposed identity, then 600 random swaps.
    pub fn init_table(&mut self) {
        let mut v = 0u8;
        for col in 0..10 {
            for row in 0..10 {
                self.table[row * 10 + col] = v;
                v += 1;
            }
        }
        for _ in 0..600 {
            let a = self.rnd(100) as usize;
            let b = self.rnd(100) as usize;
            self.table.swap(a, b);
        }
    }

    pub fn table(&self) -> &[u8; 100] {
        &self.table
    }

    /// Raw state, for saving a replay point.
    pub fn snapshot(&self) -> (u32, [u8; 100], i32) {
        (self.state, self.table, self.counter)
    }

    pub fn restore(&mut self, s: (u32, [u8; 100], i32)) {
        self.state = s.0;
        self.table = s.1;
        self.counter = s.2;
    }
}

impl Default for Rng {
    /// Deterministic default: program-start state, reseeding with a fixed 0.
    fn default() -> Rng {
        Rng::started(Seeder::fixed(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Independent MSVC rand reference.
    fn msvc(seed: u32, n: usize) -> Vec<i32> {
        let mut s = seed as u64;
        (0..n)
            .map(|_| {
                s = (s * 214013 + 2531011) % (1u64 << 32);
                ((s >> 16) & 0x7fff) as i32
            })
            .collect()
    }

    #[test]
    fn crt_rand_matches_msvc() {
        let mut r = Rng::new(Seeder::fixed(0));
        let got: Vec<i32> = (0..5).map(|_| r.crt_rand()).collect();
        // Well-known first outputs of MSVC rand() with the default seed 1.
        assert_eq!(got, vec![41, 18467, 6334, 26500, 19169]);
        r.srand(12345);
        let got: Vec<i32> = (0..1000).map(|_| r.crt_rand()).collect();
        assert_eq!(got, msvc(12345, 1000));
    }

    #[test]
    fn rnd_zero_does_not_tick() {
        let mut a = Rng::new(Seeder::fixed(7));
        let mut b = Rng::new(Seeder::fixed(7));
        assert_eq!(a.rnd(0), 0);
        assert_eq!(a.rnd(10), b.rnd(10));
        assert_eq!(a.snapshot(), b.snapshot());
    }

    #[test]
    fn reseeds_on_200th_draw() {
        let mut r = Rng::new(Seeder::fixed(99));
        let mut reference = msvc(1, 199);
        for v in reference.drain(..) {
            assert_eq!(r.rnd(32768), v);
        }
        // 200th draw: counter passes 199, srand(99) first.
        assert_eq!(r.rnd(32768), msvc(99, 1)[0]);
        assert_eq!(r.rnd(32768), msvc(99, 2)[1]);
    }

    #[test]
    fn table_is_a_permutation_and_deterministic() {
        let a = Rng::started(Seeder::counting(1_000_000));
        let b = Rng::started(Seeder::counting(1_000_000));
        assert_eq!(a.table(), b.table());
        let mut sorted = *a.table();
        sorted.sort();
        assert!(sorted.iter().enumerate().all(|(i, &v)| v as usize == i));
    }

    #[test]
    fn table_reference() {
        // Reimplementation of FUN_0046d6b5 on top of the reference generator,
        // with the reseed schedule of FUN_0046d7ca.
        let seeds = [5u32, 6, 7, 8, 9, 10, 11];
        let mut it = seeds.iter();
        let mut state = 1u64;
        let mut counter = 0;
        let mut draw = |n: u64| {
            counter += 1;
            if counter > 199 {
                state = *it.next().unwrap() as u64;
                counter = 0;
            }
            state = (state * 214013 + 2531011) % (1u64 << 32);
            (((state >> 16) & 0x7fff) % n) as usize
        };
        let mut t = [0u8; 100];
        for i in 0..100 {
            t[(i % 10) * 10 + i / 10] = i as u8;
        }
        for _ in 0..600 {
            let a = draw(100);
            let b = draw(100);
            t.swap(a, b);
        }
        let mut k = 0;
        let r = Rng::started(Seeder::new(move || {
            k += 1;
            seeds[k - 1]
        }));
        assert_eq!(r.table(), &t);
    }

    #[test]
    fn pct_reads_table() {
        let mut r = Rng::started(Seeder::fixed(3));
        let (state, table, counter) = r.snapshot();
        let v = r.pct();
        let mut s = state;
        s = s.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
        let raw = ((s >> 16) & 0x7fff) as usize;
        assert!(counter < 199);
        assert_eq!(v, table[raw % 100] as i32);
    }
}
