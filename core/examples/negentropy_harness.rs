//! Line-protocol harness for the upstream Negentropy conformance suite.
//!
//! Driven by the pinned upstream `test.pl`/`fuzz.pl`/`protoversion.pl`
//! (ADR-0010) over stdin/stdout: `item,<created>,<id-hex>` inserts an
//! item, `seal` builds the engine, `initiate` opens reconciliation, and
//! `msg,<hex>` feeds one message — answered with `have,<id>` / `need,<id>`
//! lines followed by `msg,<hex>` or `done`. The FRAMESIZELIMIT environment
//! variable caps outgoing message sizes, as the upstream harnesses do.
//! Errors abort loudly: the suite treats a dead harness as a failure.

use std::io::{BufRead, Write};

use racnet_core::store::Item;
use racnet_core::sync::negentropy::store::SealedVec;
use racnet_core::sync::negentropy::Negentropy;

fn main() {
    let frame_limit = std::env::var("FRAMESIZELIMIT")
        .ok()
        .map(|v| {
            v.parse::<usize>()
                .expect("FRAMESIZELIMIT must be an integer")
        })
        .filter(|&v| v != 0);

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    let mut items: Vec<Item> = Vec::new();
    let mut engine: Option<Negentropy<SealedVec>> = None;

    for line in stdin.lock().lines() {
        let line = line.expect("read from stdin");
        let mut fields = line.split(',');
        match fields.next() {
            Some("item") => {
                let created: u64 = fields
                    .next()
                    .expect("item needs a timestamp")
                    .parse()
                    .expect("item timestamp must be a u64");
                let id_hex = fields.next().expect("item needs an id").trim();
                let id: [u8; 32] = hex::decode(id_hex)
                    .expect("item id must be hex")
                    .try_into()
                    .expect("item id must be 32 bytes");
                items.push(Item {
                    sort_key: created,
                    id,
                });
            }
            Some("seal") => {
                let sealed = SealedVec::new(std::mem::take(&mut items));
                engine = Some(Negentropy::new(sealed, frame_limit).expect("engine construction"));
            }
            Some("initiate") => {
                let msg = engine
                    .as_mut()
                    .expect("initiate before seal")
                    .initiate()
                    .expect("initiate");
                assert_within_limit(frame_limit, msg.len());
                writeln!(out, "msg,{}", hex::encode(&msg)).expect("write to stdout");
            }
            Some("msg") => {
                let msg =
                    hex::decode(fields.next().unwrap_or("").trim()).expect("message must be hex");
                let round = engine
                    .as_mut()
                    .expect("msg before seal")
                    .reconcile(&msg)
                    .expect("reconcile");
                for id in &round.have {
                    writeln!(out, "have,{}", hex::encode(id)).expect("write to stdout");
                }
                for id in &round.need {
                    writeln!(out, "need,{}", hex::encode(id)).expect("write to stdout");
                }
                match round.reply {
                    Some(reply) => {
                        assert_within_limit(frame_limit, reply.len());
                        writeln!(out, "msg,{}", hex::encode(&reply)).expect("write to stdout");
                    }
                    None => writeln!(out, "done").expect("write to stdout"),
                }
            }
            other => panic!("unknown command: {other:?}"),
        }
        out.flush().expect("flush stdout");
    }
}

fn assert_within_limit(limit: Option<usize>, len: usize) {
    if let Some(limit) = limit {
        assert!(len <= limit, "frameSizeLimit exceeded: {len} > {limit}");
    }
}
