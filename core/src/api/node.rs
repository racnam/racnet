//! The [`Node`]: one mutex over everything a device shares across links
//! (ADR-0014).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use ed25519_dalek::SigningKey;
use zeroize::Zeroizing;

use crate::link::{HandshakeLimiter, LimiterConfig, LinkDriver, LinkDriverConfig};
use crate::noise::{Fingerprint, Keypair};
use crate::store::{EntryStore, StoreError};
use crate::sync::LinkRole;
use crate::wire::{encode_frame, Entry, GossipPush, Message};

use super::{
    entries_in_window, entry_view, fresh_ephemeral, map_event, ApiError, CloseCause, EntryView,
    Event, Identity, LinkIo, LinkOpen, LinkStatus, SyncStart, SyncWindow,
};

/// One live link and its limiter bookkeeping.
struct Link {
    driver: LinkDriver,
    /// The limiter ticket, held until the handshake completes or the
    /// link dies (responder links only).
    ticket: Option<u64>,
}

struct NodeInner {
    noise_seed: Zeroizing<[u8; 32]>,
    signing_key: SigningKey,
    store: EntryStore,
    limiter: HandshakeLimiter,
    links: HashMap<u64, Link>,
    next_link_id: u64,
    /// Monotone clamp over caller-supplied clocks: per-connection threads
    /// read the clock, then race for the mutex, so small inversions are
    /// normal and must not error.
    last_now_us: u64,
}

impl NodeInner {
    fn clamp_now(&mut self, now_us: u64) -> u64 {
        self.last_now_us = self.last_now_us.max(now_us);
        self.last_now_us
    }

    /// Retires a link, releasing its limiter ticket if it never
    /// established.
    fn retire(&mut self, link_id: u64) {
        if let Some(link) = self.links.remove(&link_id) {
            if let Some(ticket) = link.ticket {
                self.limiter.closed(ticket);
            }
        }
    }

    /// Applies limiter bookkeeping and link retirement for the events of
    /// one driver call, in order.
    fn settle(&mut self, link_id: u64, events: &[Event]) {
        for event in events {
            match event {
                Event::Established { .. } => {
                    if let Some(link) = self.links.get_mut(&link_id) {
                        if let Some(ticket) = link.ticket.take() {
                            self.limiter.established(ticket);
                        }
                    }
                }
                Event::Closed { .. } => self.retire(link_id),
                _ => {}
            }
        }
    }
}

/// A device: its identity, entry store, handshake limiter, and every
/// live link, behind one lock. See the module docs of [`crate::api`].
#[derive(uniffi::Object)]
pub struct Node {
    fingerprint: Fingerprint,
    inner: Mutex<NodeInner>,
}

impl Node {
    fn lock(&self) -> MutexGuard<'_, NodeInner> {
        // A poisoned mutex means a prior panic; the state machine itself
        // is panic-free on untrusted input, so recover rather than
        // propagate.
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn static_keys(seed: &[u8; 32]) -> Keypair {
        Keypair::from_secret_bytes(*seed)
    }
}

#[uniffi::export]
impl Node {
    /// A node from a persisted identity (see [`super::generate_identity`]).
    #[uniffi::constructor]
    pub fn new(identity: Identity) -> Result<Arc<Node>, ApiError> {
        let noise_seed: [u8; 32] = identity
            .noise_seed
            .as_slice()
            .try_into()
            .map_err(|_| ApiError::InvalidIdentity)?;
        let signing_seed: [u8; 32] = identity
            .signing_seed
            .as_slice()
            .try_into()
            .map_err(|_| ApiError::InvalidIdentity)?;
        let fingerprint = Fingerprint::of(&Self::static_keys(&noise_seed).public);
        Ok(Arc::new(Node {
            fingerprint,
            inner: Mutex::new(NodeInner {
                noise_seed: Zeroizing::new(noise_seed),
                signing_key: SigningKey::from_bytes(&signing_seed),
                store: EntryStore::new(),
                limiter: HandshakeLimiter::new(LimiterConfig::default()),
                links: HashMap::new(),
                next_link_id: 0,
                last_now_us: 0,
            }),
        }))
    }

    /// SHA-256 fingerprint of our static public key (32 bytes).
    pub fn fingerprint(&self) -> Vec<u8> {
        self.fingerprint.0.to_vec()
    }

    /// We opened a transport connection: this link's Noise initiator
    /// (PROTOCOL.md §4). Write the returned frames, then feed replies to
    /// [`on_bytes`](Node::on_bytes).
    pub fn connect(&self, now_us: u64) -> Result<LinkOpen, ApiError> {
        let ephemeral = fresh_ephemeral()?;
        let mut inner = self.lock();
        let now = inner.clamp_now(now_us);
        let mut driver = LinkDriver::new(
            LinkRole::Initiator,
            Self::static_keys(&inner.noise_seed),
            ephemeral,
            LinkDriverConfig::default(),
            now,
        );
        let out = driver.start();
        let link_id = inner.next_link_id;
        inner.next_link_id += 1;
        inner.links.insert(
            link_id,
            Link {
                driver,
                ticket: None,
            },
        );
        Ok(LinkOpen {
            link_id,
            initial_frames: out.frames,
        })
    }

    /// A remote opened a transport connection to us: this link's Noise
    /// responder. `Ok(None)` means the handshake limiter refused it
    /// (§4.5): close the connection, send nothing.
    pub fn accept(&self, remote_addr: Vec<u8>, now_us: u64) -> Result<Option<LinkOpen>, ApiError> {
        let ephemeral = fresh_ephemeral()?;
        let mut inner = self.lock();
        let now = inner.clamp_now(now_us);
        let Some(ticket) = inner.limiter.admit(&remote_addr, now) else {
            return Ok(None);
        };
        let driver = LinkDriver::new(
            LinkRole::Responder,
            Self::static_keys(&inner.noise_seed),
            ephemeral,
            LinkDriverConfig::default(),
            now,
        );
        let link_id = inner.next_link_id;
        inner.next_link_id += 1;
        inner.links.insert(
            link_id,
            Link {
                driver,
                ticket: Some(ticket),
            },
        );
        Ok(Some(LinkOpen {
            link_id,
            initial_frames: Vec::new(),
        }))
    }

    /// Raw stream bytes from a link's transport, any segmentation. Write
    /// the returned frames in order, then act on the events — a close may
    /// carry a final encrypted ERROR frame that must still be written.
    /// An unknown or already-closed link id returns empty output: close
    /// races against a reading thread are normal, not errors.
    pub fn on_bytes(&self, link_id: u64, bytes: Vec<u8>, now_us: u64) -> LinkIo {
        let mut inner = self.lock();
        let now = inner.clamp_now(now_us);
        let inner = &mut *inner;
        let Some(link) = inner.links.get_mut(&link_id) else {
            return LinkIo::default();
        };
        let out = link.driver.on_bytes(&mut inner.store, &bytes, now);
        let io = LinkIo {
            frames: out.frames,
            events: out
                .events
                .iter()
                .map(|event| map_event(link_id, event))
                .collect(),
        };
        inner.settle(link_id, &io.events);
        io
    }

    /// The transport connection died. Retires the link and reports the
    /// close exactly once; repeated or stale calls return nothing.
    pub fn on_transport_closed(&self, link_id: u64, now_us: u64) -> Vec<Event> {
        let mut inner = self.lock();
        inner.clamp_now(now_us);
        if !inner.links.contains_key(&link_id) {
            return Vec::new();
        }
        inner.retire(link_id);
        vec![Event::Closed {
            link_id,
            cause: CloseCause::TransportClosed,
        }]
    }

    /// Periodic time advancement: enforces the 24-hour lifetime cap
    /// (§4.3) and the half-open handshake timeout (§4.5). Closes are
    /// silent by protocol, so this returns events only — there is never
    /// a frame to write. Drop the transport connection of every link a
    /// `Closed` event names.
    pub fn tick(&self, now_us: u64) -> Vec<Event> {
        let mut inner = self.lock();
        let now = inner.clamp_now(now_us);
        let mut events = Vec::new();

        let link_ids: Vec<u64> = inner.links.keys().copied().collect();
        for link_id in link_ids {
            let Some(link) = inner.links.get_mut(&link_id) else {
                continue;
            };
            let out = link.driver.on_tick(now);
            let mapped: Vec<Event> = out
                .events
                .iter()
                .map(|event| map_event(link_id, event))
                .collect();
            inner.settle(link_id, &mapped);
            events.extend(mapped);
        }

        for ticket in inner.limiter.expire(now) {
            let expired = inner
                .links
                .iter()
                .find(|(_, link)| link.ticket == Some(ticket))
                .map(|(&id, _)| id);
            if let Some(link_id) = expired {
                // The ticket is already spent; remove the link directly.
                inner.links.remove(&link_id);
                events.push(Event::Closed {
                    link_id,
                    cause: CloseCause::HandshakeTimeout,
                });
            }
        }
        events
    }

    /// Point-in-time state of a link, if it is live.
    pub fn link_status(&self, link_id: u64) -> Option<LinkStatus> {
        let inner = self.lock();
        inner.links.get(&link_id).map(|link| LinkStatus {
            established: link.driver.is_established(),
            remote_fingerprint: link
                .driver
                .remote_fingerprint()
                .map(|fingerprint| fingerprint.0.to_vec()),
        })
    }

    /// Opens a reconciliation session on an established link (§6.2).
    pub fn start_sync(&self, link_id: u64, window: SyncWindow) -> Result<SyncStart, ApiError> {
        let mut inner = self.lock();
        let inner = &mut *inner;
        let link = inner.links.get_mut(&link_id).ok_or(ApiError::UnknownLink)?;
        let (session_id, out) =
            link.driver
                .open_sync(&inner.store, window.into())
                .map_err(|err| match err {
                    crate::link::LinkError::NotEstablished => ApiError::NotEstablished,
                    other => ApiError::SyncRefused {
                        reason: other.to_string(),
                    },
                })?;
        let io = LinkIo {
            frames: out.frames,
            events: out
                .events
                .iter()
                .map(|event| map_event(link_id, event))
                .collect(),
        };
        inner.settle(link_id, &io.events);
        Ok(SyncStart { session_id, io })
    }

    /// Signs and stores a new entry authored by this node. The entry
    /// syncs to peers via reconciliation sessions; call
    /// [`start_sync`](Node::start_sync) to push it proactively.
    pub fn create_entry(
        &self,
        kind: u64,
        payload: Vec<u8>,
        sort_key_ms: u64,
    ) -> Result<EntryView, ApiError> {
        let mut inner = self.lock();
        let entry = Entry::sign(&inner.signing_key, sort_key_ms, kind, payload);
        // Guard: an entry too large for one frame would poison every
        // future push that includes it, so refuse it at the door.
        encode_frame(&Message::GossipPush(GossipPush {
            entries: vec![entry.clone()],
            ttl: 0,
        }))
        .map_err(|_| ApiError::InvalidArgument {
            reason: "payload too large to frame".to_string(),
        })?;
        let view = entry_view(&entry);
        match inner.store.insert(entry) {
            Ok(_) => Ok(view),
            Err(StoreError::SortKeyReserved) => Err(ApiError::InvalidArgument {
                reason: "sort key 2^64 - 1 is reserved".to_string(),
            }),
            Err(StoreError::BadSignature) => Err(ApiError::InvalidArgument {
                reason: "entry failed self-verification".to_string(),
            }),
        }
    }

    /// The stored entries whose sort keys fall in `window`, in §6.1
    /// order.
    pub fn entries(&self, window: SyncWindow) -> Vec<EntryView> {
        let inner = self.lock();
        entries_in_window(&inner.store, window)
    }

    /// Number of entries held.
    pub fn entry_count(&self) -> u64 {
        self.lock().store.len() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn node_is_send_and_sync() {
        // UniFFI objects must be Send + Sync; keep the failure loud and
        // local when an inner type loses the property.
        assert_send_sync::<Node>();
    }

    fn identity(byte: u8) -> Identity {
        Identity {
            noise_seed: vec![byte; 32],
            signing_seed: vec![byte.wrapping_add(1); 32],
        }
    }

    #[test]
    fn seeds_must_be_exactly_32_bytes() {
        for bad in [0usize, 31, 33] {
            assert!(matches!(
                Node::new(Identity {
                    noise_seed: vec![1; bad],
                    signing_seed: vec![2; 32],
                }),
                Err(ApiError::InvalidIdentity)
            ));
            assert!(matches!(
                Node::new(Identity {
                    noise_seed: vec![1; 32],
                    signing_seed: vec![2; bad],
                }),
                Err(ApiError::InvalidIdentity)
            ));
        }
    }

    #[test]
    fn fingerprint_is_stable_for_a_seed() {
        let a = Node::new(identity(0x11)).unwrap();
        let b = Node::new(identity(0x11)).unwrap();
        assert_eq!(a.fingerprint(), b.fingerprint());
        assert_eq!(a.fingerprint().len(), 32);
        assert_ne!(
            a.fingerprint(),
            Node::new(identity(0x22)).unwrap().fingerprint()
        );
    }

    #[test]
    fn create_entry_rejects_reserved_sort_key_and_oversize_payload() {
        let node = Node::new(identity(0x11)).unwrap();
        assert!(matches!(
            node.create_entry(0, b"x".to_vec(), u64::MAX),
            Err(ApiError::InvalidArgument { .. })
        ));
        // Larger than the maximum padded inner length can ever carry.
        assert!(matches!(
            node.create_entry(0, vec![0; 64 * 1024], 1),
            Err(ApiError::InvalidArgument { .. })
        ));
        assert_eq!(node.entry_count(), 0);

        let view = node
            .create_entry(7, b"hello".to_vec(), 1_700_000_000_000)
            .unwrap();
        assert_eq!(view.kind, 7);
        assert_eq!(view.payload, b"hello");
        assert_eq!(node.entry_count(), 1);
        let listed = node.entries(SyncWindow {
            since_ms: 0,
            until_ms: u64::MAX,
        });
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, view.id);
    }

    #[test]
    fn unknown_links_are_tolerated_or_typed_errors() {
        let node = Node::new(identity(0x11)).unwrap();
        let io = node.on_bytes(99, vec![0xaa; 16], 0);
        assert!(io.frames.is_empty() && io.events.is_empty());
        assert!(node.on_transport_closed(99, 0).is_empty());
        assert!(node.link_status(99).is_none());
        assert!(matches!(
            node.start_sync(
                99,
                SyncWindow {
                    since_ms: 0,
                    until_ms: u64::MAX
                }
            ),
            Err(ApiError::UnknownLink)
        ));
    }

    #[test]
    fn transport_close_reports_exactly_once() {
        let node = Node::new(identity(0x11)).unwrap();
        let open = node.connect(0).unwrap();
        assert_eq!(open.initial_frames.len(), 1, "initiator HELLO");
        let events = node.on_transport_closed(open.link_id, 5);
        assert_eq!(
            events,
            vec![Event::Closed {
                link_id: open.link_id,
                cause: CloseCause::TransportClosed,
            }]
        );
        assert!(node.on_transport_closed(open.link_id, 6).is_empty());
    }

    #[test]
    fn non_monotonic_clocks_are_clamped_not_errors() {
        let node = Node::new(identity(0x11)).unwrap();
        let open = node.connect(1_000_000).unwrap();
        // An earlier timestamp from a racing thread must not panic, close
        // anything, or roll time back.
        let io = node.on_bytes(open.link_id, Vec::new(), 500_000);
        assert!(io.events.is_empty());
        assert!(node.tick(400_000).is_empty());
        assert!(node.link_status(open.link_id).is_some());
    }
}
