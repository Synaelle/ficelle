// SPDX-License-Identifier: AGPL-3.0-or-later

//! Content defined chunking (`FastCDC`, with normalized chunk sizes): large
//! content is cut where its bytes say, not every N bytes, so inserting a
//! few bytes moves one cut and the chunks after it stay the same. A small
//! change to a large file then stores only the chunks around it.

use std::io::Read;

use crate::error::Result;

/// Chunks are at least this long, but the last.
pub const MIN: usize = 256 * 1024;
/// The size most chunks come near.
pub const AVERAGE: usize = 1024 * 1024;
/// Chunks are at most this long.
pub const MAX: usize = 4 * 1024 * 1024;

/// `bits` one bits spread over the upper part of a word: the gear hash
/// mixes its high bits best.
const fn mask(bits: u32) -> u64 {
    let mut out = 0u64;
    let mut i = 0;
    while i < bits {
        out |= 1 << (63 - i * 48 / bits);
        i += 1;
    }
    out
}

// cut when these bits of the rolling hash are zero: about 2^20 (the
// average) tries apart, with more bits before the average (fewer cuts) and
// fewer after it, so sizes gather around it
const MASK_SMALL: u64 = mask(22);
const MASK_LARGE: u64 = mask(18);

/// One random number per byte value, fixed: the cuts must fall in the
/// same places on every device and every version.
const GEAR: [u64; 256] = {
    let mut table = [0u64; 256];
    // splitmix64 from a fixed seed
    let mut state: u64 = 0x5eed_f1ce_11e0_0001;
    let mut i = 0;
    while i < 256 {
        state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        table[i] = z ^ (z >> 31);
        i += 1;
    }
    table
};

/// Where the first chunk of `data` ends.
fn cut(data: &[u8]) -> usize {
    if data.len() <= MIN {
        return data.len();
    }
    let end = data.len().min(MAX);
    let middle = AVERAGE.min(end);
    let mut hash: u64 = 0;
    let mut i = MIN;
    while i < middle {
        hash = (hash << 1).wrapping_add(GEAR[data[i] as usize]);
        if hash & MASK_SMALL == 0 {
            return i + 1;
        }
        i += 1;
    }
    while i < end {
        hash = (hash << 1).wrapping_add(GEAR[data[i] as usize]);
        if hash & MASK_LARGE == 0 {
            return i + 1;
        }
        i += 1;
    }
    end
}

/// Reads `source` to its end and gives its chunks in order.
///
/// # Errors
///
/// Reading failed, or `each` failed.
pub fn chunks(source: &mut dyn Read, mut each: impl FnMut(&[u8]) -> Result<()>) -> Result<()> {
    // grows only as far as the content goes: a small note costs a small
    // buffer, not a chunk's worth of zeroes
    let mut buffer: Vec<u8> = Vec::new();
    let mut done = false;
    loop {
        // keep at least one whole chunk's worth in hand, so a cut is never
        // made at the end of what happened to be read
        while !done && buffer.len() < MAX {
            let want = (MAX - buffer.len()) as u64;
            if source.take(want).read_to_end(&mut buffer)? == 0 {
                done = true;
            }
        }
        if buffer.is_empty() {
            return Ok(());
        }
        let at = if done && buffer.len() <= MIN {
            buffer.len()
        } else {
            cut(&buffer)
        };
        each(&buffer[..at])?;
        buffer.drain(..at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// xorshift, a failing case fails the same way every time.
    struct Rng(u64);
    impl Rng {
        fn next_u64(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    fn bytes(seed: u64, len: usize) -> Vec<u8> {
        let mut rng = Rng(seed);
        (0..len)
            .map(|_| u8::try_from(rng.next_u64() & 0xff).unwrap())
            .collect()
    }

    fn cut_all(data: &[u8], read_size: usize) -> Vec<Vec<u8>> {
        // a reader that gives few bytes at a time, as a network does
        struct Trickle<'a>(&'a [u8], usize);
        impl Read for Trickle<'_> {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                let n = self.0.len().min(out.len()).min(self.1);
                out[..n].copy_from_slice(&self.0[..n]);
                self.0 = &self.0[n..];
                Ok(n)
            }
        }
        let mut out = Vec::new();
        chunks(&mut Trickle(data, read_size), |c| {
            out.push(c.to_vec());
            Ok(())
        })
        .unwrap();
        out
    }

    #[test]
    fn chunks_join_back_within_their_sizes_however_read() {
        let data = bytes(1, 20 * 1024 * 1024 + 123);
        let whole = cut_all(&data, usize::MAX);
        assert_eq!(whole.concat(), data);
        for (i, c) in whole.iter().enumerate() {
            assert!(c.len() <= MAX);
            assert!(
                c.len() >= MIN || i == whole.len() - 1,
                "chunk {i}: {}",
                c.len()
            );
        }
        // averages near the target
        let average = data.len() / whole.len();
        assert!((AVERAGE / 2..AVERAGE * 2).contains(&average), "{average}");
        // the cuts do not depend on how the bytes arrive
        assert_eq!(cut_all(&data, 7919), whole);
        assert!(cut_all(&[], 10).is_empty());
        assert_eq!(cut_all(b"small", 10), vec![b"small".to_vec()]);
    }

    #[test]
    fn an_insertion_changes_only_the_chunks_around_it() {
        let data = bytes(2, 30 * 1024 * 1024);
        let mut changed = data.clone();
        changed.splice(15_000_000..15_000_000, b"a few new bytes".iter().copied());
        let before: std::collections::HashSet<Vec<u8>> =
            cut_all(&data, usize::MAX).into_iter().collect();
        let after = cut_all(&changed, usize::MAX);
        let new = after.iter().filter(|c| !before.contains(*c)).count();
        assert!(new <= 2, "{new} of {} chunks changed", after.len());
    }
}
