//! Order-statistics index with composable range summaries (ADR-0011).
//!
//! An arena-allocated weight-balanced binary tree over [`Item`]s. Every
//! node carries its subtree's `(count, id-sum mod 2^256)` summary — the
//! monoid Negentropy fingerprints are built from — so insertion, rank
//! navigation, lower bounds, and range summaries are all O(log n). The set
//! is append-only (PROTOCOL.md §3.5), so the tree never deletes.

use crate::store::item::Item;
use crate::sync::negentropy::fingerprint::IdSum;

/// Sentinel index for "no child".
const NIL: u32 = u32::MAX;

/// Weight-balance parameters ⟨Δ, Γ⟩ = ⟨3, 2⟩ (Hirai & Yamamoto), the
/// integer pair proven sound for rotation-based insertion.
const DELTA: u64 = 3;
const GAMMA: u64 = 2;

#[derive(Debug, Clone)]
struct Node {
    item: Item,
    left: u32,
    right: u32,
    /// Subtree size, this node included.
    count: u32,
    /// Sum of all ids in the subtree, this node included.
    sum: IdSum,
}

/// A dynamic ordered set of [`Item`]s with rank navigation and O(log n)
/// `(count, id-sum)` summaries over arbitrary contiguous ranges.
#[derive(Debug, Clone)]
pub struct OrderIndex {
    nodes: Vec<Node>,
    root: u32,
}

impl Default for OrderIndex {
    /// An empty index; the derived impl would zero `root`, which is a
    /// valid node index, not the [`NIL`] sentinel.
    fn default() -> OrderIndex {
        OrderIndex::new()
    }
}

impl OrderIndex {
    /// An empty index.
    pub fn new() -> OrderIndex {
        OrderIndex {
            nodes: Vec::new(),
            root: NIL,
        }
    }

    /// Number of items held.
    pub fn len(&self) -> usize {
        self.subtree_count(self.root) as usize
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.root == NIL
    }

    /// Inserts an item; returns false (and changes nothing) if an equal
    /// item is already present.
    pub fn insert(&mut self, item: Item) -> bool {
        let (root, inserted) = self.insert_at(self.root, item);
        self.root = root;
        inserted
    }

    /// The item at `rank` in the total order.
    ///
    /// Panics if `rank >= len()`: callers navigate by ranks the index
    /// itself reported.
    pub fn item_at(&self, rank: usize) -> Item {
        assert!(rank < self.len(), "rank out of range");
        let mut rank = rank as u32;
        let mut idx = self.root;
        loop {
            let node = &self.nodes[idx as usize];
            let left = self.subtree_count(node.left);
            if rank < left {
                idx = node.left;
            } else if rank == left {
                return node.item;
            } else {
                rank -= left + 1;
                idx = node.right;
            }
        }
    }

    /// The first rank whose item is `>= bound`, or `len()` if none is.
    pub fn lower_bound(&self, bound: &Item) -> usize {
        let mut acc: usize = 0;
        let mut idx = self.root;
        while idx != NIL {
            let node = &self.nodes[idx as usize];
            if node.item < *bound {
                acc += self.subtree_count(node.left) as usize + 1;
                idx = node.right;
            } else {
                idx = node.left;
            }
        }
        acc
    }

    /// The `(count, id-sum)` summary of the items at ranks `range`.
    pub fn summary(&self, range: core::ops::Range<usize>) -> (u64, IdSum) {
        assert!(
            range.start <= range.end && range.end <= self.len(),
            "bad range"
        );
        let (hi_count, hi_sum) = self.prefix_summary(range.end);
        let (lo_count, mut lo_sum) = self.prefix_summary(range.start);
        lo_sum.negate();
        let mut sum = hi_sum;
        sum.combine(&lo_sum);
        (hi_count - lo_count, sum)
    }

    /// In-order iterator over the items at ranks `range`.
    pub fn iter_range(&self, range: core::ops::Range<usize>) -> RangeIter<'_> {
        assert!(
            range.start <= range.end && range.end <= self.len(),
            "bad range"
        );
        let mut iter = RangeIter {
            index: self,
            stack: Vec::new(),
            remaining: range.end - range.start,
        };
        // Seed the stack with the ancestors still to be yielded on the
        // path down to the item at range.start.
        let mut rank = range.start as u32;
        let mut idx = self.root;
        while idx != NIL {
            let node = &self.nodes[idx as usize];
            let left = self.subtree_count(node.left);
            if rank < left {
                iter.stack.push(idx);
                idx = node.left;
            } else if rank == left {
                iter.stack.push(idx);
                break;
            } else {
                rank -= left + 1;
                idx = node.right;
            }
        }
        iter
    }

    /// Summary of the first `k` items.
    fn prefix_summary(&self, k: usize) -> (u64, IdSum) {
        let mut remaining = k as u32;
        let mut count: u64 = 0;
        let mut sum = IdSum::zero();
        let mut idx = self.root;
        while idx != NIL && remaining > 0 {
            let node = &self.nodes[idx as usize];
            let left = self.subtree_count(node.left);
            if remaining <= left {
                idx = node.left;
            } else {
                if node.left != NIL {
                    let l = &self.nodes[node.left as usize];
                    count += u64::from(l.count);
                    sum.combine(&l.sum);
                }
                count += 1;
                sum.add_id(&node.item.id);
                remaining -= left + 1;
                idx = node.right;
            }
        }
        (count, sum)
    }

    fn subtree_count(&self, idx: u32) -> u32 {
        if idx == NIL {
            0
        } else {
            self.nodes[idx as usize].count
        }
    }

    /// Weight of a subtree: size + 1, so empty subtrees weigh 1.
    fn weight(&self, idx: u32) -> u64 {
        u64::from(self.subtree_count(idx)) + 1
    }

    /// Recomputes a node's summary from its children.
    fn pull_up(&mut self, idx: u32) {
        let node = &self.nodes[idx as usize];
        let (left, right) = (node.left, node.right);
        let mut count = 1;
        let mut sum = IdSum::zero();
        sum.add_id(&self.nodes[idx as usize].item.id);
        for child in [left, right] {
            if child != NIL {
                let c = &self.nodes[child as usize];
                count += c.count;
                let child_sum = c.sum;
                sum.combine(&child_sum);
            }
        }
        let node = &mut self.nodes[idx as usize];
        node.count = count;
        node.sum = sum;
    }

    fn insert_at(&mut self, idx: u32, item: Item) -> (u32, bool) {
        if idx == NIL {
            let mut sum = IdSum::zero();
            sum.add_id(&item.id);
            self.nodes.push(Node {
                item,
                left: NIL,
                right: NIL,
                count: 1,
                sum,
            });
            return ((self.nodes.len() - 1) as u32, true);
        }
        let node_item = self.nodes[idx as usize].item;
        let inserted = match item.cmp(&node_item) {
            core::cmp::Ordering::Equal => return (idx, false),
            core::cmp::Ordering::Less => {
                let (child, inserted) = self.insert_at(self.nodes[idx as usize].left, item);
                self.nodes[idx as usize].left = child;
                inserted
            }
            core::cmp::Ordering::Greater => {
                let (child, inserted) = self.insert_at(self.nodes[idx as usize].right, item);
                self.nodes[idx as usize].right = child;
                inserted
            }
        };
        if !inserted {
            return (idx, false);
        }
        self.pull_up(idx);
        (self.balance(idx), true)
    }

    /// Restores the weight-balance invariant at `idx` after one insertion
    /// below it, returning the subtree's new root.
    fn balance(&mut self, idx: u32) -> u32 {
        let (left, right) = {
            let node = &self.nodes[idx as usize];
            (node.left, node.right)
        };
        if self.weight(left) * DELTA < self.weight(right) {
            // Right-heavy.
            let r = &self.nodes[right as usize];
            let (rl, rr) = (r.left, r.right);
            if self.weight(rl) < GAMMA * self.weight(rr) {
                self.rotate_left(idx)
            } else {
                let new_right = self.rotate_right(right);
                self.nodes[idx as usize].right = new_right;
                self.pull_up(idx);
                self.rotate_left(idx)
            }
        } else if self.weight(right) * DELTA < self.weight(left) {
            // Left-heavy.
            let l = &self.nodes[left as usize];
            let (ll, lr) = (l.left, l.right);
            if self.weight(lr) < GAMMA * self.weight(ll) {
                self.rotate_right(idx)
            } else {
                let new_left = self.rotate_left(left);
                self.nodes[idx as usize].left = new_left;
                self.pull_up(idx);
                self.rotate_right(idx)
            }
        } else {
            idx
        }
    }

    /// Rotates `idx`'s right child above it; returns the new subtree root.
    fn rotate_left(&mut self, idx: u32) -> u32 {
        let right = self.nodes[idx as usize].right;
        let right_left = self.nodes[right as usize].left;
        self.nodes[idx as usize].right = right_left;
        self.nodes[right as usize].left = idx;
        self.pull_up(idx);
        self.pull_up(right);
        right
    }

    /// Rotates `idx`'s left child above it; returns the new subtree root.
    fn rotate_right(&mut self, idx: u32) -> u32 {
        let left = self.nodes[idx as usize].left;
        let left_right = self.nodes[left as usize].right;
        self.nodes[idx as usize].left = left_right;
        self.nodes[left as usize].right = idx;
        self.pull_up(idx);
        self.pull_up(left);
        left
    }
}

/// In-order iterator over a rank range of an [`OrderIndex`].
#[derive(Debug)]
pub struct RangeIter<'a> {
    index: &'a OrderIndex,
    stack: Vec<u32>,
    remaining: usize,
}

impl Iterator for RangeIter<'_> {
    type Item = Item;

    fn next(&mut self) -> Option<Item> {
        if self.remaining == 0 {
            return None;
        }
        let idx = self.stack.pop()?;
        let node = &self.index.nodes[idx as usize];
        // Queue the leftmost path of the right subtree.
        let mut child = node.right;
        while child != NIL {
            self.stack.push(child);
            child = self.index.nodes[child as usize].left;
        }
        self.remaining -= 1;
        Some(node.item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining, Some(self.remaining))
    }
}

impl ExactSizeIterator for RangeIter<'_> {}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::*;

    fn item(sort_key: u64, seed: u8) -> Item {
        Item {
            sort_key,
            id: [seed; 32],
        }
    }

    impl OrderIndex {
        /// Test-only audit: order, weight balance, and summary integrity.
        fn audit(&self) {
            self.audit_at(self.root);
            let mut prev: Option<Item> = None;
            for it in self.iter_range(0..self.len()) {
                if let Some(p) = prev {
                    assert!(p < it, "items out of order");
                }
                prev = Some(it);
            }
        }

        fn audit_at(&self, idx: u32) -> (u32, IdSum) {
            if idx == NIL {
                return (0, IdSum::zero());
            }
            let node = &self.nodes[idx as usize];
            let (lc, ls) = self.audit_at(node.left);
            let (rc, rs) = self.audit_at(node.right);
            let mut sum = ls;
            sum.add_id(&node.item.id);
            sum.combine(&rs);
            assert_eq!(node.count, lc + rc + 1, "count summary wrong");
            assert_eq!(node.sum, sum, "id-sum summary wrong");
            let (lw, rw) = (u64::from(lc) + 1, u64::from(rc) + 1);
            assert!(
                lw * DELTA >= rw && rw * DELTA >= lw,
                "weight balance violated"
            );
            (node.count, node.sum)
        }
    }

    #[test]
    fn duplicate_inserts_are_rejected() {
        let mut idx = OrderIndex::new();
        assert!(idx.insert(item(5, 1)));
        assert!(!idx.insert(item(5, 1)));
        assert_eq!(idx.len(), 1);
    }

    #[test]
    fn ascending_and_descending_inserts_stay_balanced() {
        for descending in [false, true] {
            let mut idx = OrderIndex::new();
            for i in 0..1000u64 {
                let key = if descending { 1000 - i } else { i };
                assert!(idx.insert(item(key, (i % 251) as u8)));
            }
            idx.audit();
            assert_eq!(idx.len(), 1000);
        }
    }

    proptest! {
        #[test]
        fn matches_btreeset_oracle(
            keys in proptest::collection::vec((0u64..64, 0u8..8), 0..300),
            probe_key in 0u64..64,
            probe_seed in 0u8..8,
        ) {
            let mut idx = OrderIndex::new();
            let mut oracle = BTreeSet::new();
            for (sort_key, seed) in keys {
                let it = item(sort_key, seed);
                prop_assert_eq!(idx.insert(it), oracle.insert(it));
            }
            idx.audit();
            let sorted: Vec<Item> = oracle.iter().copied().collect();
            prop_assert_eq!(idx.len(), sorted.len());

            // Rank navigation and iteration agree with the sorted oracle.
            let collected: Vec<Item> = idx.iter_range(0..idx.len()).collect();
            prop_assert_eq!(&collected, &sorted);
            for (rank, it) in sorted.iter().enumerate() {
                prop_assert_eq!(idx.item_at(rank), *it);
            }

            // lower_bound agrees with a linear scan.
            let probe = item(probe_key, probe_seed);
            let expected = sorted.iter().position(|it| *it >= probe).unwrap_or(sorted.len());
            prop_assert_eq!(idx.lower_bound(&probe), expected);

            // Range summaries agree with direct recomputation.
            let n = sorted.len();
            for (lo, hi) in [(0, n), (0, n / 2), (n / 3, n), (n / 4, 3 * n / 4)] {
                let (count, sum) = idx.summary(lo..hi);
                let mut expected_sum = IdSum::zero();
                for it in &sorted[lo..hi] {
                    expected_sum.add_id(&it.id);
                }
                prop_assert_eq!(count, (hi - lo) as u64);
                prop_assert_eq!(sum, expected_sum);
            }

            // Partial iteration honors the requested window.
            let mid: Vec<Item> = idx.iter_range(n / 4..3 * n / 4).collect();
            prop_assert_eq!(&mid[..], &sorted[n / 4..3 * n / 4]);
        }
    }
}
