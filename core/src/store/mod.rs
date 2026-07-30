//! Entry storage: signed entries plus the reconciliation order (ADR-0011).
//!
//! [`EntryStore`] owns the §3.5 rule that signatures are verified before
//! anything is stored, keeps entries content-addressed by id, and maintains
//! an [`OrderIndex`] over their `(sort-key, id)` positions so
//! reconciliation sessions can snapshot sort-key windows cheaply.

pub mod item;
pub mod order;

use std::collections::HashMap;

pub use item::{EntryId, Item};
pub use order::OrderIndex;

use crate::wire::{Entry, SortKeyWindow};

/// Errors from inserting entries into the store.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// The entry's signature failed verification (PROTOCOL.md §3.5).
    #[error("invalid entry signature")]
    BadSignature,
    /// The entry carries the reserved sort key 2^64 − 1 (PROTOCOL.md §6.1).
    #[error("sort key 2^64 - 1 is reserved")]
    SortKeyReserved,
}

/// In-memory entry store: id-addressed entries plus the §6.1 order.
#[derive(Debug, Default)]
pub struct EntryStore {
    entries: HashMap<EntryId, Entry>,
    order: OrderIndex,
}

impl EntryStore {
    /// An empty store.
    pub fn new() -> EntryStore {
        EntryStore::default()
    }

    /// Verifies and inserts an entry.
    ///
    /// Returns `Ok(false)` if an entry with the same id is already held —
    /// entries are content-addressed, so a duplicate id is a byte-identical
    /// duplicate, which is normal gossip, not an error.
    pub fn insert(&mut self, entry: Entry) -> Result<bool, StoreError> {
        if entry.sort_key == u64::MAX {
            return Err(StoreError::SortKeyReserved);
        }
        entry.verify().map_err(|_| StoreError::BadSignature)?;
        let id = entry.id();
        if self.entries.contains_key(&id) {
            return Ok(false);
        }
        self.order.insert(Item {
            sort_key: entry.sort_key,
            id,
        });
        self.entries.insert(id, entry);
        Ok(true)
    }

    /// The entry with this id, if held.
    pub fn get(&self, id: &EntryId) -> Option<&Entry> {
        self.entries.get(id)
    }

    /// Whether an entry with this id is held.
    pub fn contains(&self, id: &EntryId) -> bool {
        self.entries.contains_key(id)
    }

    /// Number of entries held.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The live order index over all held entries.
    pub fn order(&self) -> &OrderIndex {
        &self.order
    }

    /// The sorted items whose sort keys fall in `window`, copied out as the
    /// sealed set a reconciliation session works over (ADR-0011).
    ///
    /// The window is `[since, until)`. Because the reserved sort key
    /// 2^64 − 1 can never be stored, `until` = 2^64 − 1 naturally means
    /// "everything from `since`" (PROTOCOL.md §6.2) without special-casing.
    pub fn snapshot(&self, window: SortKeyWindow) -> Vec<Item> {
        let lo = self.order.lower_bound(&Item {
            sort_key: window.since,
            id: [0; 32],
        });
        let hi = self.order.lower_bound(&Item {
            sort_key: window.until,
            id: [0; 32],
        });
        self.order.iter_range(lo..hi.max(lo)).collect()
    }
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::SigningKey;

    use super::*;

    fn signed(sort_key: u64, payload: &[u8]) -> Entry {
        Entry::sign(
            &SigningKey::from_bytes(&[9u8; 32]),
            sort_key,
            0,
            payload.to_vec(),
        )
    }

    #[test]
    fn insert_verifies_and_deduplicates() {
        let mut store = EntryStore::new();
        let entry = signed(10, b"a");
        assert_eq!(store.insert(entry.clone()), Ok(true));
        assert_eq!(store.insert(entry.clone()), Ok(false));
        assert_eq!(store.len(), 1);
        assert!(store.contains(&entry.id()));
        assert_eq!(store.get(&entry.id()), Some(&entry));

        let mut forged = signed(10, b"b");
        forged.sig[0] ^= 1;
        assert_eq!(store.insert(forged), Err(StoreError::BadSignature));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn reserved_sort_key_is_rejected() {
        let mut store = EntryStore::new();
        assert_eq!(
            store.insert(signed(u64::MAX, b"a")),
            Err(StoreError::SortKeyReserved)
        );
        assert!(store.is_empty());
    }

    #[test]
    fn snapshot_honors_the_window() {
        let mut store = EntryStore::new();
        for key in [5u64, 10, 15, 20] {
            store.insert(signed(key, b"x")).unwrap();
        }
        let all = store.snapshot(SortKeyWindow {
            since: 0,
            until: u64::MAX,
        });
        assert_eq!(all.len(), 4);
        assert!(all.windows(2).all(|w| w[0] < w[1]));

        let mid = store.snapshot(SortKeyWindow {
            since: 10,
            until: 20,
        });
        assert_eq!(
            mid.iter().map(|item| item.sort_key).collect::<Vec<_>>(),
            [10, 15]
        );

        let empty = store.snapshot(SortKeyWindow {
            since: 21,
            until: 4,
        });
        assert!(empty.is_empty());
    }
}
