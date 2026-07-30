//! Storage abstraction the Negentropy engine reconciles over.
//!
//! The engine needs rank navigation, bound search, range fingerprints, and
//! in-order iteration — nothing else. [`SealedVec`] is the sealed
//! per-session snapshot (ADR-0011); the live
//! [`OrderIndex`](crate::store::OrderIndex) implements the same trait for
//! direct use and for cross-checking the two in tests.

use crate::store::{Item, OrderIndex};
use crate::sync::negentropy::bound::Bound;
use crate::sync::negentropy::fingerprint::{Fingerprint, IdSum};

/// The item set a reconciliation engine works over.
///
/// Implementations present an immutable, duplicate-free item sequence in
/// the §6.1 total order, addressed by rank.
pub trait NegentropyStore {
    /// Number of items.
    fn len(&self) -> usize;

    /// Whether there are no items.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The item at `rank`; `rank` is below [`len`](NegentropyStore::len).
    fn item_at(&self, rank: usize) -> Item;

    /// The first rank in `lo..hi` whose item is at or after `bound`, or
    /// `hi` if none is.
    fn lower_bound(&self, lo: usize, hi: usize, bound: &Bound) -> usize;

    /// The fingerprint over the items at ranks `range`.
    fn fingerprint(&self, range: core::ops::Range<usize>) -> Fingerprint;

    /// Visits items at ranks `range` in order with their ranks, stopping
    /// early when `f` returns false.
    fn for_each(&self, range: core::ops::Range<usize>, f: &mut dyn FnMut(usize, Item) -> bool);
}

/// A bound expressed as an [`Item`], comparing identically because bound
/// prefixes compare zero-padded (see [`Bound::item_is_before`]).
fn bound_as_item(bound: &Bound) -> Item {
    Item {
        sort_key: bound.ts,
        id: *bound.padded_id(),
    }
}

/// A sealed, sorted, duplicate-free item snapshot (ADR-0011).
#[derive(Debug, Clone, Default)]
pub struct SealedVec {
    items: Vec<Item>,
}

impl SealedVec {
    /// Seals an item set: sorts it and drops exact duplicates.
    pub fn new(mut items: Vec<Item>) -> SealedVec {
        items.sort_unstable();
        items.dedup();
        SealedVec { items }
    }
}

impl NegentropyStore for SealedVec {
    fn len(&self) -> usize {
        self.items.len()
    }

    fn item_at(&self, rank: usize) -> Item {
        self.items[rank]
    }

    fn lower_bound(&self, lo: usize, hi: usize, bound: &Bound) -> usize {
        let key = bound_as_item(bound);
        lo + self.items[lo..hi].partition_point(|item| *item < key)
    }

    fn fingerprint(&self, range: core::ops::Range<usize>) -> Fingerprint {
        let mut sum = IdSum::zero();
        for item in &self.items[range.clone()] {
            sum.add_id(&item.id);
        }
        sum.fingerprint(range.len() as u64)
    }

    fn for_each(&self, range: core::ops::Range<usize>, f: &mut dyn FnMut(usize, Item) -> bool) {
        for (rank, item) in self.items[range.clone()].iter().enumerate() {
            if !f(range.start + rank, *item) {
                break;
            }
        }
    }
}

impl NegentropyStore for OrderIndex {
    fn len(&self) -> usize {
        OrderIndex::len(self)
    }

    fn item_at(&self, rank: usize) -> Item {
        OrderIndex::item_at(self, rank)
    }

    fn lower_bound(&self, lo: usize, hi: usize, bound: &Bound) -> usize {
        OrderIndex::lower_bound(self, &bound_as_item(bound)).clamp(lo, hi)
    }

    fn fingerprint(&self, range: core::ops::Range<usize>) -> Fingerprint {
        let (count, sum) = self.summary(range);
        sum.fingerprint(count)
    }

    fn for_each(&self, range: core::ops::Range<usize>, f: &mut dyn FnMut(usize, Item) -> bool) {
        for (rank, item) in self.iter_range(range.clone()).enumerate() {
            if !f(range.start + rank, item) {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::negentropy::bound::TS_INFINITY;

    fn item(sort_key: u64, seed: u8) -> Item {
        Item {
            sort_key,
            id: [seed; 32],
        }
    }

    fn stores() -> (SealedVec, OrderIndex) {
        let items: Vec<Item> = (0..100u64).map(|i| item(i / 2, (i % 251) as u8)).collect();
        let mut index = OrderIndex::new();
        for it in &items {
            index.insert(*it);
        }
        (SealedVec::new(items), index)
    }

    #[test]
    fn sealed_vec_sorts_and_dedups() {
        let sealed = SealedVec::new(vec![item(2, 1), item(1, 1), item(2, 1)]);
        assert_eq!(sealed.len(), 2);
        assert_eq!(sealed.item_at(0), item(1, 1));
        assert_eq!(sealed.item_at(1), item(2, 1));
    }

    #[test]
    fn sealed_vec_and_order_index_agree() {
        let (sealed, index) = stores();
        let n = NegentropyStore::len(&sealed);
        assert_eq!(n, NegentropyStore::len(&index));
        for rank in 0..n {
            assert_eq!(
                NegentropyStore::item_at(&sealed, rank),
                NegentropyStore::item_at(&index, rank)
            );
        }
        for (lo, hi) in [(0, n), (10, 90), (n / 2, n / 2)] {
            assert_eq!(sealed.fingerprint(lo..hi), index.fingerprint(lo..hi));
            for bound in [
                Bound::from_ts(0),
                Bound::from_ts(25),
                Bound::at_item(30, &[7; 32]),
                Bound::infinity(),
                Bound::from_ts(TS_INFINITY - 1),
            ] {
                assert_eq!(
                    NegentropyStore::lower_bound(&sealed, lo, hi, &bound),
                    NegentropyStore::lower_bound(&index, lo, hi, &bound),
                );
            }
        }
        let mut seen = Vec::new();
        NegentropyStore::for_each(&index, 5..15, &mut |rank, it| {
            seen.push((rank, it));
            seen.len() < 4
        });
        assert_eq!(seen.len(), 4);
        assert_eq!(seen[0], (5, NegentropyStore::item_at(&sealed, 5)));
    }
}
