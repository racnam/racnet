//! Integration tests: the wire codec over the simulated transport, and the
//! simulation's determinism guarantees.

use racnet_core::sim::{Delivery, LinkConfig, LinkKind, SimNet};
use racnet_core::wire::{encode_frame, ErrorMsg, FrameDecoder, Hello, Message};

fn ble_stream() -> LinkConfig {
    LinkConfig {
        latency_us: 5_000,
        mtu: 23,
        kind: LinkKind::Stream,
        loss_per_mille: 0,
    }
}

#[test]
fn frames_survive_mtu_23_stream_segmentation() {
    let mut net = SimNet::new(42);
    let a = net.add_node();
    let b = net.add_node();
    net.connect(a, b, ble_stream());

    let hello = Message::Hello(Hello {
        versions: vec![1],
        features: 0,
        transports: vec![1],
    });
    let error = Message::Error(ErrorMsg { code: 2 });
    net.send(a, b, &encode_frame(&hello).unwrap()).unwrap();
    net.send(a, b, &encode_frame(&error).unwrap()).unwrap();
    net.run_until_idle();

    let mut dec = FrameDecoder::new();
    for delivery in net.take_inbox(b) {
        match delivery {
            Delivery::Data { from, bytes } => {
                assert_eq!(from, a);
                dec.push(&bytes);
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(dec.next_message().unwrap(), Some(hello));
    assert_eq!(dec.next_message().unwrap(), Some(error));
    assert_eq!(dec.next_message().unwrap(), None);
}

#[test]
fn datagram_loss_is_deterministic_per_seed() {
    let run = |seed: u64| -> Vec<Delivery> {
        let mut net = SimNet::new(seed);
        let a = net.add_node();
        let b = net.add_node();
        net.connect(
            a,
            b,
            LinkConfig {
                latency_us: 1_000,
                mtu: 64,
                kind: LinkKind::Datagram,
                loss_per_mille: 500,
            },
        );
        for i in 0..100u8 {
            net.send(a, b, &[i]).unwrap();
        }
        net.run_until_idle();
        net.take_inbox(b)
    };

    let first = run(7);
    let second = run(7);
    assert_eq!(first, second, "same seed must give an identical trace");
    assert!(
        !first.is_empty() && first.len() < 100,
        "loss of 500‰ should drop some but not all of 100 datagrams, delivered {}",
        first.len()
    );
}

#[test]
fn partition_blocks_datagrams_until_heal() {
    let mut net = SimNet::new(3);
    let a = net.add_node();
    let b = net.add_node();
    net.connect(
        a,
        b,
        LinkConfig {
            latency_us: 0,
            mtu: 64,
            kind: LinkKind::Datagram,
            loss_per_mille: 0,
        },
    );

    net.partition(&[&[a], &[b]]);
    net.send(a, b, &[1]).unwrap();
    net.run_until_idle();
    assert_eq!(net.take_inbox(b), vec![]);

    net.heal();
    net.send(a, b, &[2]).unwrap();
    net.run_until_idle();
    assert_eq!(
        net.take_inbox(b),
        vec![Delivery::Data {
            from: a,
            bytes: vec![2]
        }]
    );
}

#[test]
fn stream_loss_drops_the_link_not_bytes() {
    let mut net = SimNet::new(9);
    let a = net.add_node();
    let b = net.add_node();
    net.connect(
        a,
        b,
        LinkConfig {
            latency_us: 1_000,
            mtu: 23,
            kind: LinkKind::Stream,
            loss_per_mille: 1_000,
        },
    );

    assert!(net.send(a, b, &[0; 10]).is_err());
    net.run_until_idle();
    assert_eq!(net.take_inbox(b), vec![Delivery::LinkDown { peer: a }]);
    assert_eq!(net.take_inbox(a), vec![Delivery::LinkDown { peer: b }]);
    assert!(net.send(a, b, &[0; 10]).is_err(), "link stays down");
}
