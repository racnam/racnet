//! Reconciliation sessions over wire messages (PROTOCOL.md §6).
//!
//! [`Syncer`] manages the RECON_INIT / RECON_MSG / RECON_DONE flow for one
//! link: concurrent directional sessions keyed by who opened them, session
//! ids with the §6.2 parity rule (transport initiator even, acceptor odd),
//! a sealed snapshot per session (ADR-0011), and entry transfer of
//! reconciled differences via GOSSIP_PUSH. It is transport-agnostic —
//! it consumes and produces [`Message`] values and never touches a radio.

use std::collections::{BTreeSet, HashMap};

use crate::store::{EntryId, EntryStore, StoreError};
use crate::sync::negentropy::store::SealedVec;
use crate::sync::negentropy::Negentropy;
use crate::sync::SyncError;
use crate::wire::{GossipPush, Message, ReconDone, ReconInit, ReconMsg, SortKeyWindow};

/// Byte budget for the entries in one GOSSIP_PUSH batch: safely below the
/// u16 payload-length ceiling (PROTOCOL.md §1.2) after CBOR overhead.
const PUSH_BATCH_BUDGET: usize = 32 * 1024;

/// Which side of the transport connection this syncer is (PROTOCOL.md
/// §6.2): the initiator opens sessions with even ids, the acceptor odd.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkRole {
    /// We initiated the transport connection; our session ids are even.
    Initiator,
    /// We accepted the transport connection; our session ids are odd.
    Responder,
}

/// Tunables for a [`Syncer`].
#[derive(Debug, Clone, Copy)]
pub struct SyncConfig {
    /// Frame-size limit handed to each session's Negentropy engine.
    pub frame_limit: Option<usize>,
    /// Hop budget on pushed entries; 0 means the peer must not relay.
    pub push_ttl: u64,
    /// Maximum concurrent open sessions, both directions combined.
    pub max_sessions: usize,
}

impl Default for SyncConfig {
    /// Frame limit at the engine floor, direct (no-relay) pushes, and a
    /// small session cap.
    fn default() -> SyncConfig {
        SyncConfig {
            frame_limit: Some(crate::sync::negentropy::FRAME_LIMIT_FLOOR),
            push_ttl: 0,
            max_sessions: 8,
        }
    }
}

/// Something the sync layer learned or did while handling a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyncEvent {
    /// A pushed entry was verified and stored.
    Stored(EntryId),
    /// A pushed entry was already held; ignored.
    DuplicateIgnored(EntryId),
    /// A session round reported differences: `have` is what we hold and
    /// the peer lacks (already queued as pushes), `need` what the peer
    /// holds and we lack (it arrives via the peer's own sessions).
    Reconciled {
        /// Session id of the reporting session.
        sid: u64,
        /// Newly discovered entries only we hold.
        have: Vec<EntryId>,
        /// Newly discovered entries only the peer holds.
        need: Vec<EntryId>,
    },
    /// A session closed, by us or by the peer.
    SessionClosed {
        /// The session's id.
        sid: u64,
        /// Whether we had opened it.
        opened_by_us: bool,
    },
}

/// Messages to send and events to surface after handling one message.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncOutput {
    /// Messages to transmit to the peer, in order.
    pub replies: Vec<Message>,
    /// What happened, for the application and for tests.
    pub events: Vec<SyncEvent>,
}

/// One open reconciliation session.
#[derive(Debug)]
struct Session {
    engine: Negentropy<SealedVec>,
    /// Ids already pushed to the peer, so continuation rounds cannot
    /// queue an entry twice.
    pushed: BTreeSet<EntryId>,
}

/// Sync-session manager for one link.
#[derive(Debug)]
pub struct Syncer {
    role: LinkRole,
    config: SyncConfig,
    /// Open sessions keyed by (opened by us, sid); §6.2 sid parity makes
    /// the direction recoverable from an incoming sid.
    sessions: HashMap<(bool, u64), Session>,
    next_sid: u64,
}

impl Syncer {
    /// A syncer for a link on which we play `role`.
    pub fn new(role: LinkRole, config: SyncConfig) -> Syncer {
        Syncer {
            role,
            config,
            sessions: HashMap::new(),
            next_sid: match role {
                LinkRole::Initiator => 0,
                LinkRole::Responder => 1,
            },
        }
    }

    /// Number of currently open sessions, both directions.
    pub fn open_sessions(&self) -> usize {
        self.sessions.len()
    }

    /// Opens a reconciliation session over `window`: snapshots the store,
    /// allocates the next sid of our parity, and returns the RECON_INIT
    /// to send.
    pub fn open_session(
        &mut self,
        store: &EntryStore,
        window: SortKeyWindow,
    ) -> Result<(u64, Message), SyncError> {
        if self.sessions.len() >= self.config.max_sessions {
            return Err(SyncError::ResourceLimit);
        }
        let sealed = SealedVec::new(store.snapshot(window));
        let mut engine = Negentropy::new(sealed, self.config.frame_limit)?;
        let msg = engine.initiate()?;
        let sid = self.next_sid;
        self.next_sid += 2;
        self.sessions.insert(
            (true, sid),
            Session {
                engine,
                pushed: BTreeSet::new(),
            },
        );
        Ok((sid, Message::ReconInit(ReconInit { sid, window, msg })))
    }

    /// Ends a session we opened before it completes, returning the
    /// RECON_DONE to send, or `None` if the sid is not open.
    pub fn abort_session(&mut self, sid: u64) -> Option<Message> {
        self.sessions.remove(&(true, sid))?;
        Some(Message::ReconDone(ReconDone { sid }))
    }

    /// Handles one incoming sync message: GOSSIP_PUSH, RECON_INIT,
    /// RECON_MSG, or RECON_DONE.
    ///
    /// On error the caller reports [`SyncError::error_code`] per §7 and
    /// closes the link.
    pub fn handle_message(
        &mut self,
        store: &mut EntryStore,
        msg: &Message,
    ) -> Result<SyncOutput, SyncError> {
        match msg {
            Message::GossipPush(push) => self.handle_push(store, push),
            Message::ReconInit(init) => self.handle_init(store, init),
            Message::ReconMsg(rm) => self.handle_recon_msg(store, rm),
            Message::ReconDone(done) => self.handle_done(done),
            _ => Err(SyncError::Violation("message type not for the sync layer")),
        }
    }

    fn handle_push(
        &mut self,
        store: &mut EntryStore,
        push: &GossipPush,
    ) -> Result<SyncOutput, SyncError> {
        let mut output = SyncOutput::default();
        for entry in &push.entries {
            let id = entry.id();
            match store.insert(entry.clone()) {
                Ok(true) => output.events.push(SyncEvent::Stored(id)),
                Ok(false) => output.events.push(SyncEvent::DuplicateIgnored(id)),
                Err(StoreError::BadSignature) => {
                    return Err(SyncError::Violation("pushed entry signature invalid"))
                }
                Err(StoreError::SortKeyReserved) => {
                    return Err(SyncError::Violation(
                        "pushed entry uses the reserved sort key",
                    ))
                }
            }
        }
        Ok(output)
    }

    fn handle_init(
        &mut self,
        store: &mut EntryStore,
        init: &ReconInit,
    ) -> Result<SyncOutput, SyncError> {
        if init.sid % 2 != self.peer_parity() {
            return Err(SyncError::Violation("session id has the sender's parity"));
        }
        if self.sessions.contains_key(&(false, init.sid)) {
            return Err(SyncError::Violation("session id already open"));
        }
        if self.sessions.len() >= self.config.max_sessions {
            return Err(SyncError::ResourceLimit);
        }
        let sealed = SealedVec::new(store.snapshot(init.window));
        let mut engine = Negentropy::new(sealed, self.config.frame_limit)?;
        let round = engine.reconcile(&init.msg)?;
        let reply = round.reply.expect("a non-initiating engine always replies");
        self.sessions.insert(
            (false, init.sid),
            Session {
                engine,
                pushed: BTreeSet::new(),
            },
        );
        Ok(SyncOutput {
            replies: vec![Message::ReconMsg(ReconMsg {
                sid: init.sid,
                msg: reply,
            })],
            events: Vec::new(),
        })
    }

    fn handle_recon_msg(
        &mut self,
        store: &mut EntryStore,
        rm: &ReconMsg,
    ) -> Result<SyncOutput, SyncError> {
        let key = self.session_key(rm.sid);
        let session = self
            .sessions
            .get_mut(&key)
            .ok_or(SyncError::Violation("no open session with this id"))?;
        let round = session.engine.reconcile(&rm.msg)?;

        let mut output = SyncOutput::default();
        let (opened_by_us, sid) = (key.0, key.1);

        // Only sessions we opened learn differences; queue pushes for the
        // entries the peer lacks, once each.
        let have: Vec<EntryId> = round
            .have
            .into_iter()
            .filter(|id| session.pushed.insert(*id))
            .collect();
        let need: Vec<EntryId> = {
            let mut seen = BTreeSet::new();
            round
                .need
                .into_iter()
                .filter(|id| seen.insert(*id))
                .collect()
        };
        if !have.is_empty() || !need.is_empty() {
            output.events.push(SyncEvent::Reconciled {
                sid,
                have: have.clone(),
                need,
            });
            output
                .replies
                .extend(push_batches(store, &have, self.config.push_ttl));
        }

        match round.reply {
            Some(reply) => output
                .replies
                .push(Message::ReconMsg(ReconMsg { sid, msg: reply })),
            None => {
                // Initiator finished: announce completion and close.
                self.sessions.remove(&key);
                output.replies.push(Message::ReconDone(ReconDone { sid }));
                output
                    .events
                    .push(SyncEvent::SessionClosed { sid, opened_by_us });
            }
        }
        Ok(output)
    }

    fn handle_done(&mut self, done: &ReconDone) -> Result<SyncOutput, SyncError> {
        let key = self.session_key(done.sid);
        if self.sessions.remove(&key).is_none() {
            return Err(SyncError::Violation("no open session with this id"));
        }
        Ok(SyncOutput {
            replies: Vec::new(),
            events: vec![SyncEvent::SessionClosed {
                sid: done.sid,
                opened_by_us: key.0,
            }],
        })
    }

    /// Recovers the session-table key for an incoming sid via the §6.2
    /// parity rule.
    fn session_key(&self, sid: u64) -> (bool, u64) {
        (sid % 2 == self.our_parity(), sid)
    }

    fn our_parity(&self) -> u64 {
        match self.role {
            LinkRole::Initiator => 0,
            LinkRole::Responder => 1,
        }
    }

    fn peer_parity(&self) -> u64 {
        1 - self.our_parity()
    }
}

/// Splits pushed entries into GOSSIP_PUSH batches under the byte budget.
fn push_batches(store: &EntryStore, ids: &[EntryId], ttl: u64) -> Vec<Message> {
    let mut messages = Vec::new();
    let mut batch = Vec::new();
    let mut batch_bytes = 0usize;
    for id in ids {
        // The ids come from a snapshot of this store and entries are never
        // removed, so the lookup only fails if callers mix stores; skipping
        // is safer than panicking on a hostile-adjacent path.
        let Some(entry) = store.get(id) else { continue };
        let size = entry.to_bytes().len();
        if !batch.is_empty() && batch_bytes + size > PUSH_BATCH_BUDGET {
            messages.push(Message::GossipPush(GossipPush {
                entries: std::mem::take(&mut batch),
                ttl,
            }));
            batch_bytes = 0;
        }
        batch.push(entry.clone());
        batch_bytes += size;
    }
    if !batch.is_empty() {
        messages.push(Message::GossipPush(GossipPush {
            entries: batch,
            ttl,
        }));
    }
    messages
}

#[cfg(test)]
mod tests {
    use ed25519_dalek::SigningKey;

    use super::*;
    use crate::wire::Entry;

    fn entry(seed: u8, sort_key: u64) -> Entry {
        Entry::sign(
            &SigningKey::from_bytes(&[seed; 32]),
            sort_key,
            0,
            vec![seed],
        )
    }

    fn full_window() -> SortKeyWindow {
        SortKeyWindow {
            since: 0,
            until: u64::MAX,
        }
    }

    /// Delivers messages between two syncers until both go quiet.
    fn pump(
        a: &mut Syncer,
        a_store: &mut EntryStore,
        b: &mut Syncer,
        b_store: &mut EntryStore,
        first: Vec<Message>,
    ) {
        let mut to_b = first;
        let mut to_a: Vec<Message> = Vec::new();
        for _ in 0..256 {
            if to_b.is_empty() && to_a.is_empty() {
                return;
            }
            let mut next_b = Vec::new();
            for msg in to_a.drain(..) {
                next_b.extend(a.handle_message(a_store, &msg).expect("a handles").replies);
            }
            for msg in to_b.drain(..) {
                to_a.extend(b.handle_message(b_store, &msg).expect("b handles").replies);
            }
            to_b = next_b;
        }
        panic!("message pump did not settle");
    }

    #[test]
    fn sid_parity_follows_link_role() {
        let store = EntryStore::new();
        let mut initiator = Syncer::new(LinkRole::Initiator, SyncConfig::default());
        let mut responder = Syncer::new(LinkRole::Responder, SyncConfig::default());
        let (sid_a, _) = initiator.open_session(&store, full_window()).unwrap();
        let (sid_b, _) = initiator.open_session(&store, full_window()).unwrap();
        let (sid_c, _) = responder.open_session(&store, full_window()).unwrap();
        assert_eq!((sid_a, sid_b, sid_c), (0, 2, 1));
    }

    #[test]
    fn wrong_parity_and_unknown_sids_are_violations() {
        let mut store = EntryStore::new();
        let mut responder = Syncer::new(LinkRole::Responder, SyncConfig::default());
        // A RECON_INIT with an odd sid claims the responder's own parity.
        let bad_init = Message::ReconInit(ReconInit {
            sid: 3,
            window: full_window(),
            msg: vec![0x61, 0x00, 0x00, 0x02, 0x00],
        });
        assert!(matches!(
            responder.handle_message(&mut store, &bad_init),
            Err(SyncError::Violation(_))
        ));
        // A RECON_MSG for a session nobody opened.
        let orphan = Message::ReconMsg(ReconMsg {
            sid: 0,
            msg: vec![0x61],
        });
        assert!(matches!(
            responder.handle_message(&mut store, &orphan),
            Err(SyncError::Violation(_))
        ));
    }

    #[test]
    fn duplicate_session_ids_are_violations() {
        let mut store = EntryStore::new();
        let mut responder = Syncer::new(LinkRole::Responder, SyncConfig::default());
        let init = Message::ReconInit(ReconInit {
            sid: 0,
            window: full_window(),
            msg: vec![0x61, 0x00, 0x00, 0x02, 0x00],
        });
        responder.handle_message(&mut store, &init).unwrap();
        assert!(matches!(
            responder.handle_message(&mut store, &init),
            Err(SyncError::Violation(_))
        ));
    }

    #[test]
    fn session_cap_is_enforced() {
        let store = EntryStore::new();
        let config = SyncConfig {
            max_sessions: 1,
            ..SyncConfig::default()
        };
        let mut syncer = Syncer::new(LinkRole::Initiator, config);
        syncer.open_session(&store, full_window()).unwrap();
        assert_eq!(
            syncer.open_session(&store, full_window()).unwrap_err(),
            SyncError::ResourceLimit
        );
    }

    #[test]
    fn bad_pushed_entries_are_violations() {
        let mut store = EntryStore::new();
        let mut syncer = Syncer::new(LinkRole::Initiator, SyncConfig::default());
        let mut forged = entry(1, 10);
        forged.payload = vec![0xFF];
        let push = Message::GossipPush(GossipPush {
            entries: vec![forged],
            ttl: 0,
        });
        assert!(matches!(
            syncer.handle_message(&mut store, &push),
            Err(SyncError::Violation(_))
        ));
    }

    #[test]
    fn two_syncers_converge_and_close_their_sessions() {
        let mut a_store = EntryStore::new();
        let mut b_store = EntryStore::new();
        for i in 0..30 {
            a_store.insert(entry(i, u64::from(i) * 10)).unwrap();
        }
        for i in 20..50 {
            b_store.insert(entry(i, u64::from(i) * 10)).unwrap();
        }

        let mut a = Syncer::new(LinkRole::Initiator, SyncConfig::default());
        let mut b = Syncer::new(LinkRole::Responder, SyncConfig::default());

        // Both directions must run for both sides to converge.
        let (_, a_init) = a.open_session(&a_store, full_window()).unwrap();
        let (_, b_init) = b.open_session(&b_store, full_window()).unwrap();
        pump(&mut a, &mut a_store, &mut b, &mut b_store, vec![a_init]);
        pump(&mut b, &mut b_store, &mut a, &mut a_store, vec![b_init]);

        assert_eq!(a_store.len(), 50);
        assert_eq!(b_store.len(), 50);
        for i in 0..50 {
            let id = entry(i, u64::from(i) * 10).id();
            assert!(a_store.contains(&id) && b_store.contains(&id));
        }
        assert_eq!(a.open_sessions(), 0);
        assert_eq!(b.open_sessions(), 0);
    }

    #[test]
    fn abort_emits_recon_done_and_forgets_the_session() {
        let store = EntryStore::new();
        let mut syncer = Syncer::new(LinkRole::Initiator, SyncConfig::default());
        let (sid, _) = syncer.open_session(&store, full_window()).unwrap();
        let done = syncer.abort_session(sid).expect("open session aborts");
        assert_eq!(done, Message::ReconDone(ReconDone { sid }));
        assert_eq!(syncer.open_sessions(), 0);
        assert_eq!(syncer.abort_session(sid), None);
    }
}
