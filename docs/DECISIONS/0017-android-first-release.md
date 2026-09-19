# ADR 0017 — Android first release

Status: accepted for the maintainer-requested Android-first scope.

Use the existing signed-entry protocol for a public nearby message board.
Application kind 1 contains plain UTF-8, limited to 4096 bytes by the app.
Kind 0 remains diagnostic data. Author labels use the entry's signing public
key, not the unrelated Noise peer fingerprint. This introduces no wire change.
Messages are public to every peer; link encryption does not make them private.

Persist entries in a versioned append-only core journal. Each length-prefixed
canonical CBOR record is verified on reload. Flush a record before making it
visible in the in-memory index. Recover incomplete final writes by truncating
only the incomplete tail; reject malformed complete records and preserve the
file. Exclusively lock the journal to prevent two writers. Limit storage to
64 MiB and 10,000 entries; fail visibly rather than silently evicting messages.
The core uses std filesystem APIs; Android alone uses libc for file locking. Android supplies a
path in its private files directory; identity remains Keystore-wrapped, and
message bodies are not separately encrypted at rest. Backup stays disabled.

Reconciliation snapshots are immutable, so changes during a session require
another session. Keep at most one local session per link, coalesce pending
changes, and retry refused starts on ticks. Received entries dirty other live
links, supporting eventual relay without claiming the deferred M6 routing
algorithms. Bound the transport write queue; close and reconcile again after
backpressure rather than accumulating unlimited memory.

Hardware acceptance is deferred, not waived. The build is a sideload preview;
store distribution and any stronger security claims remain maintainer choices.

Revise ADR-0016's silent identity rotation: an unreadable Keystore envelope
now stops startup and preserves the data. Use Android AtomicFile for durable
identity replacement. The same JNA version already used by the application
is added as a test-only desktop runtime jar so JVM integration tests exercise
the actual host Rust library, not a mock of the node.

Android-specific dependency: `libc` supplies `flock` because Rust 1.97's
`File::try_lock` returns Unsupported on Android. This was reproduced during
API 35 emulator startup. Other platforms use std locking. The File owns the
descriptor and releases the lock on close. libc was already in Cargo.lock;
no independent lock protocol or stale lockfile recovery is introduced.
