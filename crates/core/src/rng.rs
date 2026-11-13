//! Deterministic random numbers.
//!
//! A run is fully decided by its seed: the same seed gives the same maps, encounters and rewards.
//! Each purpose (map, encounters, rewards, ...) draws from its own stream, so buying something in a
//! shop does not change which boss waits three floors later.

use serde::{Deserialize, Serialize};

/// xoshiro256** seeded through SplitMix64. The whole state is saved with the run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rng {
    s: [u64; 4],
}

fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        let mut sm = seed;
        let s = [
            splitmix64(&mut sm),
            splitmix64(&mut sm),
            splitmix64(&mut sm),
            splitmix64(&mut sm),
        ];
        Rng { s }
    }

    /// An independent stream for one purpose. `index` separates repeated uses (the act number, say).
    pub fn stream(seed: u64, purpose: &str, index: u64) -> Self {
        // FNV-1a over the purpose name.
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for b in purpose.bytes() {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        let mut mix = seed ^ h.rotate_left(17);
        mix = splitmix64(&mut mix) ^ index.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        Rng::new(mix)
    }

    pub fn next_u64(&mut self) -> u64 {
        let result = self.s[1].wrapping_mul(5).rotate_left(7).wrapping_mul(9);
        let t = self.s[1] << 17;
        self.s[2] ^= self.s[0];
        self.s[3] ^= self.s[1];
        self.s[1] ^= self.s[2];
        self.s[0] ^= self.s[3];
        self.s[2] ^= t;
        self.s[3] = self.s[3].rotate_left(45);
        result
    }

    /// A number in `0..n`, without modulo bias. `n` must be above zero; `below(0)` returns 0.
    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        // Lemire's method.
        let mut x = (self.next_u64() >> 32) as u32;
        let mut m = u64::from(x) * u64::from(n);
        let mut low = m as u32;
        if low < n {
            let threshold = n.wrapping_neg() % n;
            while low < threshold {
                x = (self.next_u64() >> 32) as u32;
                m = u64::from(x) * u64::from(n);
                low = m as u32;
            }
        }
        (m >> 32) as u32
    }

    /// A number in `lo..=hi`. The bounds may come in either order.
    pub fn range(&mut self, lo: u32, hi: u32) -> u32 {
        let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        let span = hi - lo;
        if span == u32::MAX {
            return (self.next_u64() >> 32) as u32;
        }
        lo + self.below(span + 1)
    }

    /// True `pct` times out of a hundred. 0 or less is never, 100 or more is always.
    pub fn chance(&mut self, pct: u32) -> bool {
        self.below(100) < pct
    }

    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.below(i as u32 + 1) as usize;
            items.swap(i, j);
        }
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() {
            None
        } else {
            Some(&items[self.below(items.len() as u32) as usize])
        }
    }

    /// The index of one weight, chosen in proportion. `None` when every weight is zero.
    pub fn weighted(&mut self, weights: &[u32]) -> Option<usize> {
        let total: u64 = weights.iter().map(|w| u64::from(*w)).sum();
        if total == 0 || total > u64::from(u32::MAX) {
            return None;
        }
        let mut roll = u64::from(self.below(total as u32));
        for (i, w) in weights.iter().enumerate() {
            let w = u64::from(*w);
            if roll < w {
                return Some(i);
            }
            roll -= w;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix_matches_reference() {
        // First output of SplitMix64 from state 0 (Vigna's reference implementation).
        let mut state = 0u64;
        assert_eq!(splitmix64(&mut state), 0xE220_A839_7B1D_CDAF);
    }

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn streams_differ_by_purpose_and_index() {
        let mut a = Rng::stream(7, "map", 0);
        let mut b = Rng::stream(7, "rewards", 0);
        let mut c = Rng::stream(7, "map", 1);
        let (x, y, z) = (a.next_u64(), b.next_u64(), c.next_u64());
        assert_ne!(x, y);
        assert_ne!(x, z);
        assert_ne!(y, z);
    }

    #[test]
    fn below_stays_in_range_and_covers_it() {
        let mut rng = Rng::new(1);
        let mut seen = [false; 7];
        for _ in 0..2000 {
            let v = rng.below(7);
            assert!(v < 7);
            seen[v as usize] = true;
        }
        assert!(seen.iter().all(|s| *s));
        assert_eq!(rng.below(0), 0);
        assert_eq!(rng.below(1), 0);
    }

    #[test]
    fn below_is_roughly_uniform() {
        let mut rng = Rng::new(99);
        let mut counts = [0u32; 5];
        for _ in 0..50_000 {
            counts[rng.below(5) as usize] += 1;
        }
        for c in counts {
            assert!(
                (9_300..=10_700).contains(&c),
                "bucket {c} is far from 10000"
            );
        }
    }

    #[test]
    fn range_handles_bounds() {
        let mut rng = Rng::new(3);
        for _ in 0..500 {
            let v = rng.range(5, 9);
            assert!((5..=9).contains(&v));
            assert_eq!(rng.range(4, 4), 4);
            let w = rng.range(9, 5);
            assert!((5..=9).contains(&w));
        }
        let _ = rng.range(0, u32::MAX);
    }

    #[test]
    fn chance_extremes() {
        let mut rng = Rng::new(5);
        for _ in 0..200 {
            assert!(!rng.chance(0));
            assert!(rng.chance(100));
            assert!(rng.chance(250));
        }
    }

    #[test]
    fn shuffle_keeps_the_items() {
        let mut rng = Rng::new(11);
        let mut v: Vec<u32> = (0..50).collect();
        rng.shuffle(&mut v);
        let mut sorted = v.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..50).collect::<Vec<_>>());
        assert_ne!(v, (0..50).collect::<Vec<_>>());
    }

    #[test]
    fn weighted_respects_zero_weights() {
        let mut rng = Rng::new(13);
        for _ in 0..500 {
            let i = rng.weighted(&[0, 3, 0, 1]).unwrap();
            assert!(i == 1 || i == 3);
        }
        assert_eq!(rng.weighted(&[0, 0]), None);
        assert_eq!(rng.weighted(&[]), None);
    }

    #[test]
    fn state_survives_a_save() {
        let mut rng = Rng::new(77);
        rng.next_u64();
        let saved = serde_json::to_string(&rng).unwrap();
        let mut loaded: Rng = serde_json::from_str(&saved).unwrap();
        assert_eq!(rng.next_u64(), loaded.next_u64());
    }
}
