//! The reconciliation order over stored entries (PROTOCOL.md §6.1).

/// An entry id: SHA-256 of the canonical entry encoding (PROTOCOL.md §3.5).
pub type EntryId = [u8; 32];

/// An entry's position in the reconciliation order: its sort key and id.
///
/// The derived `Ord` is the §6.1 total order — ascending `sort_key`, ties
/// broken by ascending bytewise id comparison — because the fields are
/// declared in that order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Item {
    /// Author-claimed milliseconds since the Unix epoch (PROTOCOL.md §6.1).
    pub sort_key: u64,
    /// The entry id.
    pub id: EntryId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_is_sort_key_then_id() {
        let a = Item {
            sort_key: 1,
            id: [0xFF; 32],
        };
        let b = Item {
            sort_key: 2,
            id: [0x00; 32],
        };
        let c = Item {
            sort_key: 2,
            id: [0x01; 32],
        };
        assert!(a < b && b < c);
    }
}
