use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use racnet_core::api::{Identity, Node, SyncWindow};
use racnet_core::store::EntryStore;

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "racnet-store-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self) -> PathBuf {
        self.0.join("entries")
    }
    fn node(&self) -> std::sync::Arc<Node> {
        Node::open(identity(), self.path().to_str().unwrap().into()).unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn identity() -> Identity {
    Identity {
        noise_seed: vec![1; 32],
        signing_seed: vec![2; 32],
    }
}
fn all() -> SyncWindow {
    SyncWindow {
        since_ms: 0,
        until_ms: u64::MAX,
    }
}

#[test]
fn entries_and_identity_survive_restart_and_exclusive_writer_is_enforced() {
    let temp = Temp::new();
    let node = temp.node();
    let entry = node
        .create_entry(1, b"offline message".to_vec(), 42)
        .unwrap();
    let fingerprint = node.fingerprint();
    assert!(Node::open(identity(), temp.path().to_str().unwrap().into()).is_err());
    drop(node);
    let reopened = temp.node();
    assert_eq!(reopened.fingerprint(), fingerprint);
    assert_eq!(reopened.entry_count(), 1);
    assert_eq!(reopened.entries(all())[0].id, entry.id);
    assert_eq!(reopened.entries(all())[0].payload, b"offline message");
    reopened.create_entry(1, b"second".to_vec(), 43).unwrap();
    drop(reopened);
    assert_eq!(temp.node().entry_count(), 2);
}

#[test]
fn every_incomplete_record_tail_recovers_without_losing_prior_messages() {
    let temp = Temp::new();
    let node = temp.node();
    node.create_entry(1, b"first".to_vec(), 1).unwrap();
    let boundary = fs::metadata(temp.path()).unwrap().len() as usize;
    node.create_entry(1, b"second".to_vec(), 2).unwrap();
    drop(node);
    let bytes = fs::read(temp.path()).unwrap();
    for end in boundary..bytes.len() {
        fs::write(temp.path(), &bytes[..end]).unwrap();
        let reopened = temp.node();
        assert_eq!(reopened.entry_count(), 1, "cut at {end}");
        assert_eq!(fs::metadata(temp.path()).unwrap().len(), boundary as u64);
        reopened
            .create_entry(1, b"replacement".to_vec(), 3)
            .unwrap();
        drop(reopened);
        assert_eq!(temp.node().entry_count(), 2);
    }
}

#[test]
fn complete_corrupt_records_are_preserved_and_rejected() {
    let temp = Temp::new();
    let node = temp.node();
    node.create_entry(1, b"message".to_vec(), 1).unwrap();
    drop(node);
    let mut bytes = fs::read(temp.path()).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    fs::write(temp.path(), &bytes).unwrap();
    assert!(EntryStore::open(&temp.path()).is_err());
    assert_eq!(fs::read(temp.path()).unwrap(), bytes);
}

#[test]
fn oversized_length_and_bad_header_fail_without_allocating_or_truncating() {
    let temp = Temp::new();
    drop(temp.node());
    OpenOptions::new()
        .append(true)
        .open(temp.path())
        .unwrap()
        .write_all(&u32::MAX.to_le_bytes())
        .unwrap();
    let bytes = fs::read(temp.path()).unwrap();
    assert!(EntryStore::open(&temp.path()).is_err());
    assert_eq!(fs::read(temp.path()).unwrap(), bytes);
    fs::write(temp.path(), b"unknown!").unwrap();
    assert!(EntryStore::open(&temp.path()).is_err());
}

#[test]
fn disk_capacity_is_checked_before_reading_records() {
    let temp = Temp::new();
    drop(temp.node());
    OpenOptions::new()
        .write(true)
        .open(temp.path())
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    assert!(matches!(
        EntryStore::open(&temp.path()),
        Err(racnet_core::store::StoreError::Capacity)
    ));
}
