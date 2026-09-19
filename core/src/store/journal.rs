//! Durable append-only entries. The in-memory index remains the sync engine's view.
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;

use super::{EntryStore, StoreError};
use crate::wire::Entry;

const MAGIC: &[u8; 8] = b"RACNET01";
const MAX_RECORD: usize = 1_048_576;
const MAX_BYTES: u64 = 64 * 1024 * 1024;
pub(super) const MAX_ENTRIES: usize = 10_000;

#[derive(Debug)]
pub(super) struct Journal {
    file: File,
    len: u64,
}

fn io(error: impl std::fmt::Display) -> StoreError {
    StoreError::Storage(error.to_string())
}

#[cfg(not(target_os = "android"))]
fn lock(file: &File) -> Result<(), StoreError> {
    file.try_lock().map_err(|err| match err {
        std::fs::TryLockError::Error(error) => io(error),
        std::fs::TryLockError::WouldBlock => io("entry journal is already open"),
    })
}

#[cfg(target_os = "android")]
fn lock(file: &File) -> Result<(), StoreError> {
    use std::os::fd::AsRawFd;
    // SAFETY: the live File owns this descriptor, flock retains no pointer,
    // and closing File releases the advisory lock. Android's std backend
    // reports Unsupported for File::try_lock on Rust 1.97.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        Ok(())
    } else {
        Err(io(std::io::Error::last_os_error()))
    }
}

impl Journal {
    pub fn open(path: &Path) -> Result<EntryStore, StoreError> {
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(io)?;
        lock(&file)?;
        let mut len = file.metadata().map_err(io)?.len();
        if len > MAX_BYTES {
            return Err(StoreError::Capacity);
        }
        if len == 0 {
            file.write_all(MAGIC).map_err(io)?;
            file.sync_all().map_err(io)?;
            // Persist the new directory entry as well as the file contents.
            if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                File::open(parent)
                    .and_then(|dir| dir.sync_all())
                    .map_err(io)?;
            }
            len = MAGIC.len() as u64;
        }
        file.seek(SeekFrom::Start(0)).map_err(io)?;
        let mut magic = [0; 8];
        file.read_exact(&mut magic).map_err(io)?;
        if &magic != MAGIC {
            return Err(io("unrecognized entry journal"));
        }
        let mut store = EntryStore::new();
        let mut offset = MAGIC.len() as u64;
        while offset < len {
            if len - offset < 4 {
                break;
            }
            let mut size = [0; 4];
            file.read_exact(&mut size).map_err(io)?;
            let size = u32::from_le_bytes(size) as usize;
            if size == 0 || size > MAX_RECORD {
                return Err(io("invalid journal record length"));
            }
            if len - offset - 4 < size as u64 {
                break;
            }
            let mut bytes = vec![0; size];
            file.read_exact(&mut bytes).map_err(io)?;
            let entry: Entry = minicbor::decode(&bytes).map_err(io)?;
            if entry.to_bytes() != bytes {
                return Err(io("noncanonical journal record"));
            }
            if store.len() >= MAX_ENTRIES {
                return Err(StoreError::Capacity);
            }
            store.insert(entry)?;
            offset += 4 + size as u64;
        }
        if offset != len {
            file.set_len(offset).map_err(io)?;
            file.sync_all().map_err(io)?;
        }
        file.seek(SeekFrom::Start(offset)).map_err(io)?;
        store.journal = Some(Journal { file, len: offset });
        Ok(store)
    }

    pub fn append(&mut self, entry: &Entry) -> Result<(), StoreError> {
        let bytes = entry.to_bytes();
        if bytes.len() > MAX_RECORD || self.len + 4 + bytes.len() as u64 > MAX_BYTES {
            return Err(StoreError::Capacity);
        }
        // Restore the last committed boundary before retrying a failed write.
        self.file.set_len(self.len).map_err(io)?;
        self.file.seek(SeekFrom::Start(self.len)).map_err(io)?;
        self.file
            .write_all(&(bytes.len() as u32).to_le_bytes())
            .map_err(io)?;
        self.file.write_all(&bytes).map_err(io)?;
        self.file.sync_all().map_err(io)?;
        self.len += 4 + bytes.len() as u64;
        Ok(())
    }
}
