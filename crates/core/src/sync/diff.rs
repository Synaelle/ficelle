// SPDX-License-Identifier: AGPL-3.0-or-later

//! A small diff for the conflict check (`super::conflict`): which items two
//! sequences share, in order, along a shortest edit between them. Myers'
//! linear space algorithm ("An O(ND) Difference Algorithm and Its
//! Variations", 1986): each step finds the middle of a shortest edit and
//! splits there, so memory stays proportional to the inputs.

/// The pairs `(i, j)` with `a[i] == b[j]` kept by a shortest edit from `a`
/// to `b`, both indexes increasing.
pub(crate) fn matches<T: PartialEq>(a: &[T], b: &[T]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    // every diagonal `middle` can reach, see there
    let size = 2 * ((a.len() + b.len()).div_ceil(2) + 1) + 1;
    let (mut forward, mut backward) = (vec![0; size], vec![0; size]);
    split(a, b, (0, 0), &mut forward, &mut backward, &mut found);
    found
}

/// Matches `a` against `b`, both starting at `at` in the full inputs.
fn split<T: PartialEq>(
    a: &[T],
    b: &[T],
    at: (usize, usize),
    forward: &mut [usize],
    backward: &mut [usize],
    found: &mut Vec<(usize, usize)>,
) {
    // a shared start and end need no search
    let start = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    found.extend((0..start).map(|k| (at.0 + k, at.1 + k)));
    let (a, b) = (&a[start..], &b[start..]);
    let at = (at.0 + start, at.1 + start);
    let end = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let (a, b) = (&a[..a.len() - end], &b[..b.len() - end]);
    if !a.is_empty() && !b.is_empty() {
        let (x, y) = middle(a, b, forward, backward);
        split(&a[..x], &b[..y], at, forward, backward, found);
        split(
            &a[x..],
            &b[y..],
            (at.0 + x, at.1 + y),
            forward,
            backward,
            found,
        );
    }
    found.extend((0..end).map(|k| (at.0 + a.len() + k, at.1 + b.len() + k)));
}

/// A point on a shortest edit from `a` to `b`, both non-empty and differing
/// at their first and last items, found by searching from both ends until
/// the two searches meet. Diagonal `k` holds the points where `x - y == k`,
/// stored at `k + offset`; `backward` measures from the ends.
#[expect(clippy::many_single_char_names, reason = "the names of Myers' paper")]
fn middle<T: PartialEq>(
    a: &[T],
    b: &[T],
    forward: &mut [usize],
    backward: &mut [usize],
) -> (usize, usize) {
    let (n, m) = (a.len(), b.len());
    let max = (n + m).div_ceil(2) + 1;
    let offset = max.cast_signed();
    let delta = n.cast_signed() - m.cast_signed();
    let odd = delta % 2 != 0;
    let at = |k: isize| (k + offset).cast_unsigned();
    forward[at(1)] = 0;
    backward[at(1)] = 0;
    for d in 0..max.cast_signed() {
        for k in (-d..=d).step_by(2) {
            let mut x = if k == -d || (k != d && forward[at(k - 1)] < forward[at(k + 1)]) {
                forward[at(k + 1)]
            } else {
                forward[at(k - 1)] + 1
            };
            let (x0, y0) = (x, (x.cast_signed() - k).cast_unsigned());
            let mut y = y0;
            while x < n && y < m && a[x] == b[y] {
                x += 1;
                y += 1;
            }
            forward[at(k)] = x;
            let other = -(k - delta);
            if odd && (k - delta).abs() < d && x + backward[at(other)] >= n {
                return (x0, y0);
            }
        }
        for k in (-d..=d).step_by(2) {
            let mut x = if k == -d || (k != d && backward[at(k - 1)] < backward[at(k + 1)]) {
                backward[at(k + 1)]
            } else {
                backward[at(k - 1)] + 1
            };
            let mut y = (x.cast_signed() - k).cast_unsigned();
            while x < n && y < m && a[n - x - 1] == b[m - y - 1] {
                x += 1;
                y += 1;
            }
            backward[at(k)] = x;
            let other = -(k - delta);
            if !odd && (k - delta).abs() <= d && x + forward[at(other)] >= n {
                return (n - x, m - y);
            }
        }
    }
    unreachable!("two searches over finite inputs always meet")
}

#[cfg(test)]
mod tests {
    use super::matches;

    /// The longest common subsequence's length, the slow sure way.
    fn lcs(a: &[u8], b: &[u8]) -> usize {
        let mut row = vec![0; b.len() + 1];
        for x in a {
            let mut diagonal = 0;
            for (j, y) in b.iter().enumerate() {
                let above = row[j + 1];
                row[j + 1] = if x == y {
                    diagonal + 1
                } else {
                    above.max(row[j])
                };
                diagonal = above;
            }
        }
        row[b.len()]
    }

    fn check(a: &[u8], b: &[u8]) {
        let found = matches(a, b);
        assert_eq!(found.len(), lcs(a, b), "{a:?} {b:?}: {found:?}");
        for (k, &(i, j)) in found.iter().enumerate() {
            assert_eq!(a[i], b[j], "{a:?} {b:?}: {found:?}");
            if k > 0 {
                let (pi, pj) = found[k - 1];
                assert!(i > pi && j > pj, "{a:?} {b:?}: {found:?}");
            }
        }
    }

    #[test]
    fn small_cases() {
        for (a, b) in [
            ("", ""),
            ("abc", ""),
            ("", "abc"),
            ("abc", "abc"),
            ("abcabba", "cbabac"),
            ("a", "b"),
            ("ab", "ba"),
            ("xaxbx", "ab"),
            ("kitten", "sitting"),
        ] {
            check(a.as_bytes(), b.as_bytes());
        }
    }

    #[test]
    fn a_shortest_edit_for_many_random_pairs() {
        // a fixed small generator: the same pairs on every run
        let mut seed = 0x2545_f491_u32;
        let mut next = move |below: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed % below
        };
        for _ in 0..3000 {
            let alphabet = 2 + next(4);
            let (la, lb) = (next(14), next(14));
            let mut word = |len: u32| -> Vec<u8> {
                (0..len)
                    .map(|_| b'a' + u8::try_from(next(alphabet)).unwrap_or(0))
                    .collect()
            };
            let a = word(la);
            let b = word(lb);
            check(&a, &b);
        }
    }
}
