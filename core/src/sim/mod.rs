//! Deterministic in-process simulated transport: the test harness for
//! protocol logic (latency, loss, partition, MTU), with no radio and no
//! wall clock.
//!
//! The simulation is a discrete-event loop over virtual time. All
//! randomness comes from a seeded [`SplitMix64`]; a run is bit-reproducible
//! from its seed on every platform.
//!
//! Two link kinds model the transports honestly: [`LinkKind::Stream`] is a
//! reliable ordered byte stream (BLE L2CAP CoC) whose MTU segments writes —
//! exercising the frame decoder's partial-read path — and where loss
//! manifests as the link dropping, since a reliable transport loses
//! connections, not bytes. [`LinkKind::Datagram`] delivers whole messages
//! with per-message loss (BLE advertisements).

use std::cmp::Reverse;
use std::collections::BinaryHeap;

/// SplitMix64 (public domain): deterministic, cross-platform, and small
/// enough to keep an RNG framework out of the mesh core. Not cryptographic;
/// test harness only.
struct SplitMix64(u64);

impl SplitMix64 {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// A node in the simulated network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(usize);

/// Transport model for a link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// Reliable ordered byte stream; MTU segments writes, loss drops the link.
    Stream,
    /// Unreliable whole-message delivery; loss drops individual messages.
    Datagram,
}

/// Configuration of one link.
#[derive(Debug, Clone, Copy)]
pub struct LinkConfig {
    /// One-way delivery latency in virtual microseconds.
    pub latency_us: u64,
    /// Maximum transmission unit in bytes.
    pub mtu: usize,
    /// Transport model.
    pub kind: LinkKind,
    /// Loss probability in permille (0–1000). Per message on datagram
    /// links; per send as a link-drop on stream links.
    pub loss_per_mille: u16,
}

/// Something a node received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    /// Bytes from a peer: one datagram, or one segment of a stream write.
    Data {
        /// Sending node.
        from: NodeId,
        /// Received bytes.
        bytes: Vec<u8>,
    },
    /// The stream link to `peer` dropped.
    LinkDown {
        /// The other end of the dropped link.
        peer: NodeId,
    },
}

/// Errors returned by [`SimNet::send`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SimError {
    /// No link exists between the two nodes.
    #[error("no link between nodes")]
    NoLink,
    /// The stream link has dropped.
    #[error("link is down")]
    LinkDown,
    /// A datagram exceeds the link MTU.
    #[error("message of {0} bytes exceeds link mtu")]
    TooLarge(usize),
}

#[derive(Debug, PartialEq, Eq)]
enum EventKind {
    Data { from: NodeId, bytes: Vec<u8> },
    LinkDown { peer: NodeId },
}

#[derive(Debug, PartialEq, Eq)]
struct Event {
    at_us: u64,
    seq: u64,
    to: NodeId,
    link: usize,
    kind: EventKind,
}

impl Ord for Event {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (self.at_us, self.seq).cmp(&(other.at_us, other.seq))
    }
}

impl PartialOrd for Event {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

struct Link {
    a: NodeId,
    b: NodeId,
    config: LinkConfig,
    up: bool,
}

/// A deterministic simulated network.
pub struct SimNet {
    now_us: u64,
    rng: SplitMix64,
    inboxes: Vec<Vec<Delivery>>,
    links: Vec<Link>,
    events: BinaryHeap<Reverse<Event>>,
    seq: u64,
    groups: Option<Vec<Option<usize>>>,
}

impl SimNet {
    /// Creates an empty network; all randomness derives from `seed`.
    pub fn new(seed: u64) -> SimNet {
        SimNet {
            now_us: 0,
            rng: SplitMix64(seed),
            inboxes: Vec::new(),
            links: Vec::new(),
            events: BinaryHeap::new(),
            seq: 0,
            groups: None,
        }
    }

    /// The current virtual time in microseconds.
    pub fn now_us(&self) -> u64 {
        self.now_us
    }

    /// Adds a node.
    pub fn add_node(&mut self) -> NodeId {
        self.inboxes.push(Vec::new());
        NodeId(self.inboxes.len() - 1)
    }

    /// Connects two nodes with a link. Reconnecting an existing pair
    /// replaces the link's configuration and brings it back up.
    pub fn connect(&mut self, a: NodeId, b: NodeId, config: LinkConfig) {
        if let Some(link) = self
            .links
            .iter_mut()
            .find(|l| (l.a == a && l.b == b) || (l.a == b && l.b == a))
        {
            link.config = config;
            link.up = true;
        } else {
            self.links.push(Link {
                a,
                b,
                config,
                up: true,
            });
        }
    }

    /// Splits the network into isolated groups. Nodes in different groups —
    /// or in no group — cannot reach each other until [`heal`](Self::heal).
    pub fn partition(&mut self, groups: &[&[NodeId]]) {
        let mut assignment = vec![None; self.inboxes.len()];
        for (g, members) in groups.iter().enumerate() {
            for node in *members {
                assignment[node.0] = Some(g);
            }
        }
        self.groups = Some(assignment);
    }

    /// Removes any partition.
    pub fn heal(&mut self) {
        self.groups = None;
    }

    fn reachable(&self, a: NodeId, b: NodeId) -> bool {
        match &self.groups {
            None => true,
            Some(assignment) => match (assignment[a.0], assignment[b.0]) {
                (Some(ga), Some(gb)) => ga == gb,
                _ => false,
            },
        }
    }

    fn schedule(&mut self, at_us: u64, to: NodeId, link: usize, kind: EventKind) {
        self.seq += 1;
        self.events.push(Reverse(Event {
            at_us,
            seq: self.seq,
            to,
            link,
            kind,
        }));
    }

    /// Sends `bytes` from `from` to `to` over their link.
    ///
    /// On a stream link the bytes are segmented to the MTU and delivered in
    /// order; on a datagram link they are delivered whole or not at all.
    pub fn send(&mut self, from: NodeId, to: NodeId, bytes: &[u8]) -> Result<(), SimError> {
        let idx = self
            .links
            .iter()
            .position(|l| (l.a == from && l.b == to) || (l.a == to && l.b == from))
            .ok_or(SimError::NoLink)?;
        let (kind, latency, mtu, loss) = {
            let l = &self.links[idx];
            if l.config.kind == LinkKind::Stream && !l.up {
                return Err(SimError::LinkDown);
            }
            (
                l.config.kind,
                l.config.latency_us,
                l.config.mtu,
                l.config.loss_per_mille,
            )
        };
        let arrival = self.now_us + latency;
        let lost = loss > 0 && (self.rng.next_u64() % 1000) < u64::from(loss);
        match kind {
            LinkKind::Datagram => {
                if bytes.len() > mtu {
                    return Err(SimError::TooLarge(bytes.len()));
                }
                if !lost {
                    self.schedule(
                        arrival,
                        to,
                        idx,
                        EventKind::Data {
                            from,
                            bytes: bytes.to_vec(),
                        },
                    );
                }
            }
            LinkKind::Stream => {
                if lost || !self.reachable(from, to) {
                    self.links[idx].up = false;
                    self.schedule(arrival, to, idx, EventKind::LinkDown { peer: from });
                    self.schedule(arrival, from, idx, EventKind::LinkDown { peer: to });
                    return Err(SimError::LinkDown);
                }
                for segment in bytes.chunks(mtu) {
                    self.schedule(
                        arrival,
                        to,
                        idx,
                        EventKind::Data {
                            from,
                            bytes: segment.to_vec(),
                        },
                    );
                }
            }
        }
        Ok(())
    }

    /// Processes the next event, advancing virtual time. Returns `false`
    /// when no events remain.
    pub fn step(&mut self) -> bool {
        let Some(Reverse(event)) = self.events.pop() else {
            return false;
        };
        self.now_us = event.at_us;
        match event.kind {
            EventKind::Data { from, bytes } => {
                let deliverable = match self.links[event.link].config.kind {
                    // In-flight data crossing a partition is lost; on a
                    // stream that also drops the link.
                    LinkKind::Datagram => self.reachable(from, event.to),
                    LinkKind::Stream => {
                        if self.reachable(from, event.to) && self.links[event.link].up {
                            true
                        } else {
                            if self.links[event.link].up {
                                self.links[event.link].up = false;
                                self.inboxes[event.to.0].push(Delivery::LinkDown { peer: from });
                                self.inboxes[from.0].push(Delivery::LinkDown { peer: event.to });
                            }
                            false
                        }
                    }
                };
                if deliverable {
                    self.inboxes[event.to.0].push(Delivery::Data { from, bytes });
                }
            }
            EventKind::LinkDown { peer } => {
                self.inboxes[event.to.0].push(Delivery::LinkDown { peer });
            }
        }
        true
    }

    /// Runs until no events remain.
    pub fn run_until_idle(&mut self) {
        while self.step() {}
    }

    /// Removes and returns everything `node` has received so far.
    pub fn take_inbox(&mut self, node: NodeId) -> Vec<Delivery> {
        std::mem::take(&mut self.inboxes[node.0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream_config() -> LinkConfig {
        LinkConfig {
            latency_us: 5_000,
            mtu: 23,
            kind: LinkKind::Stream,
            loss_per_mille: 0,
        }
    }

    #[test]
    fn splitmix64_reference_values() {
        // First three outputs for seed 0, per the public-domain reference.
        let mut rng = SplitMix64(0);
        assert_eq!(rng.next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(rng.next_u64(), 0x6E78_9E6A_A1B9_65F4);
        assert_eq!(rng.next_u64(), 0x06C4_5D18_8009_454F);
    }

    #[test]
    fn stream_segments_to_mtu_and_preserves_order() {
        let mut net = SimNet::new(1);
        let a = net.add_node();
        let b = net.add_node();
        net.connect(a, b, stream_config());
        net.send(a, b, &[0x55; 50]).unwrap();
        net.run_until_idle();
        let deliveries = net.take_inbox(b);
        assert_eq!(deliveries.len(), 3); // 23 + 23 + 4
        let bytes: Vec<u8> = deliveries
            .iter()
            .flat_map(|d| match d {
                Delivery::Data { bytes, .. } => bytes.clone(),
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(bytes, vec![0x55; 50]);
        assert_eq!(net.now_us(), 5_000);
    }

    #[test]
    fn send_without_link_fails() {
        let mut net = SimNet::new(1);
        let a = net.add_node();
        let b = net.add_node();
        assert_eq!(net.send(a, b, &[0]), Err(SimError::NoLink));
    }

    #[test]
    fn oversized_datagram_is_rejected() {
        let mut net = SimNet::new(1);
        let a = net.add_node();
        let b = net.add_node();
        net.connect(
            a,
            b,
            LinkConfig {
                latency_us: 0,
                mtu: 8,
                kind: LinkKind::Datagram,
                loss_per_mille: 0,
            },
        );
        assert_eq!(net.send(a, b, &[0; 9]), Err(SimError::TooLarge(9)));
    }
}
