# Racnet Wire Protocol

**Spec version: 0.1.0 · wire protocol version 1**

This document is the source of truth for the Racnet wire protocol. Where any
implementation disagrees with this document, the implementation is wrong.
Changes to this document are their own versioned commits and land before the
code that implements them.

The key words MUST, MUST NOT, SHOULD, and MAY are to be interpreted as
described in RFC 2119.

Conventions: all multi-byte integers outside CBOR are big-endian (network
byte order). CBOR is big-endian by construction. Byte counts are in octets.
Non-obvious choices in this document are recorded as ADRs 0003–0007 in
`docs/DECISIONS/`.

## 1. Framing

Protocol data is exchanged over a reliable, ordered byte stream (in the first
deployments, a BLE L2CAP connection-oriented channel; the transport below
this spec is out of scope). There are two layers: the **outer frame**, which
delimits messages on the stream, and the **inner message**, which carries a
typed payload and the padding that blunts traffic analysis.

A link passes through two epochs (§4): the **cleartext epoch**, where each
outer frame body is a padded inner message directly, and the **transport
epoch**, where each outer frame body is a Noise transport message whose
plaintext is a padded inner message. Padding therefore always sits inside
the encryption boundary: pad-then-encrypt, never encrypt-then-pad.

### 1.1 Outer frame

| Field  | Size (octets) | Encoding  | Description                        |
|--------|---------------|-----------|------------------------------------|
| `len`  | 2             | u16 BE    | Length of `body`; MUST NOT be 0    |
| `body` | `len`         | raw bytes | Padded inner message (cleartext epoch) or Noise message (transport epoch) |

The outer frame deliberately carries no magic bytes and no version field.
A fixed identifying constant would make every frame trivially
fingerprintable on the air, and versioning is negotiated in the first
message of every link (§5), which also versions the framing itself. The
2-octet length bounds frames at 65 535 octets, matching the Noise message
size limit; no larger frame can ever be needed.

Frames arrive back-to-back on the stream with no interleaving. Decoders
MUST tolerate arbitrary segmentation of the stream (a frame may arrive
across any number of reads).

### 1.2 Inner message

| Field         | Size (octets)  | Encoding | Description                       |
|---------------|----------------|----------|-----------------------------------|
| `msg_type`    | 1              | u8       | Message type from the registry (§2) |
| `payload_len` | 2              | u16 BE   | Length of `payload`               |
| `payload`     | `payload_len`  | CBOR     | Deterministic CBOR per the schema for `msg_type` (§3) |
| `padding`     | to block bound | zeros    | See §1.3                          |

### 1.3 Padding

The inner message (header, payload, and padding together) is padded to a
fixed block size so that frame lengths reveal only a coarse size class:

| Unpadded length `n` (octets) | Padded length              |
|------------------------------|----------------------------|
| `n` ≤ 256                    | 256                        |
| 256 < `n` ≤ 512              | 512                        |
| 512 < `n` ≤ 1024             | 1024                       |
| 1024 < `n` ≤ 2048            | 2048                       |
| 2048 < `n` ≤ 63 488          | next multiple of 2048      |
| `n` > 63 488                 | invalid; MUST NOT be sent  |

The maximum padded length is 63 488 octets (31 × 2048), chosen so that the
transport-epoch ciphertext — padded plaintext plus the 16-octet AEAD tag —
stays within the 65 535-octet Noise message limit.

Senders MUST pad to the smallest permitted padded length and MUST fill
padding with zero octets. Receivers MUST NOT interpret padding contents,
and MUST reject an inner message whose total length is not exactly the
padded length required for its `payload_len` (over-padding would otherwise
be a covert channel and defeat the size classes). Rejection is a protocol
violation (§7).

### 1.4 Fragmentation

There is none at this layer in protocol version 1. The transport is a
reliable ordered stream with its own segmentation, and no message exceeds
one frame. Application content too large for one frame (file chunks) is
split by the layer that produces it, using message types reserved in §2.

## 2. Message types

| Value       | Name        | Status in version 1                          |
|-------------|-------------|----------------------------------------------|
| `0x00`      | —           | invalid, never assigned                      |
| `0x01`      | HELLO       | active (§3.2, §5)                            |
| `0x02`      | HANDSHAKE   | active (§3.3, §4)                            |
| `0x10`      | GOSSIP_PUSH | active (§3.4)                                |
| `0x11`      | IHAVE       | reserved: Plumtree lazy announce             |
| `0x12`–`0x13` | —         | reserved: Plumtree control (graft/prune)     |
| `0x20`      | RECON_INIT  | active (§6)                                  |
| `0x21`      | RECON_MSG   | active (§6)                                  |
| `0x22`      | RECON_DONE  | active (§6)                                  |
| `0x30`–`0x3F` | —         | reserved: chunk transfer                     |
| `0x70`      | ERROR       | active (§7)                                  |
| `0xF0`–`0xFF` | —         | private use; MUST NOT appear on version 1 links |
| all others  | —           | unassigned                                   |

`0x11` is reserved now so that the future Plumtree eager/lazy forwarding
split can be introduced without a wire break; likewise `0x12`–`0x13` and
the chunk-transfer block.

Receiving a message whose type is invalid, reserved, unassigned, or
private-use is a protocol violation (§7). Forward compatibility is provided
by version negotiation (§5) and by ignorable CBOR map keys (§3.1), not by
skipping unknown message types: a parser of untrusted radio input is strict
about everything it has not agreed to accept.

## 3. Payload schemas

### 3.1 Encoding rules

All payloads are CBOR (RFC 8949), restricted as follows:

- Core deterministic encoding (RFC 8949 §4.2.1): shortest-form integer
  encodings, definite lengths only, map keys sorted bytewise ascending.
- All map keys are unsigned integers.
- No floating-point values, no tags, no indefinite-length items.
- A payload is exactly one CBOR item; trailing bytes inside `payload_len`
  are a protocol violation.

Senders MUST emit deterministic encodings. Receivers MAY reject
non-deterministic encodings, and MUST ignore unknown map keys in the
map-shaped payloads below — new optional keys are the additive extension
mechanism that needs no version bump. The `entry` array (§3.5) is
positional and fixed: it is never extended within a wire version, and
receivers MUST reject an `entry` whose array length is not exactly 5 —
tolerated extra elements would be a covert channel, exactly as over-padding
would be (§1.3). Occurrence constraints in the CDDL are normative:
a payload violating them (an empty `versions` or `entries` list) MUST be
rejected as a protocol violation.

The schemas, in CDDL (RFC 8610):

```cddl
hello = {
  0: [+ uint],            ; protocol versions the sender can speak
  1: uint,                ; feature bitset; 0 in version 1
  2: [* uint],            ; transport registry values (§3.2)
}

handshake = {
  0: bstr,                ; one Noise handshake message (§4)
}

gossip-push = {
  0: [+ entry],           ; entries being pushed
  1: uint,                ; ttl: remaining hop budget
}

recon-init = {
  0: uint,                ; sid: reconciliation session id
  1: [since: uint, until: uint],  ; sort-key window (§6.2)
  2: bstr,                ; first Negentropy message, opaque (§6.3)
}

recon-msg = {
  0: uint,                ; sid
  1: bstr,                ; Negentropy message, opaque (§6.3)
}

recon-done = {
  0: uint,                ; sid
}

error = {
  0: uint,                ; error code (§7)
}

entry = [
  author: bstr .size 32,  ; Ed25519 public key
  sort-key: uint,         ; u64; milliseconds since the Unix epoch (§6.1)
  kind: uint,             ; application-level entry kind
  payload: bstr,          ; application payload, opaque to this layer
  sig: bstr .size 64,     ; Ed25519 signature over entry-tbs
]

entry-tbs = [
  author: bstr .size 32,
  sort-key: uint,
  kind: uint,
  payload: bstr,
]
```

### 3.2 HELLO

The first message on every link, in both directions (§4, §5). `transports`
lists the transport registry values the sender can accept a data plane on:

| Value | Transport            |
|-------|----------------------|
| `1`   | BLE L2CAP CoC        |
| others | unassigned          |

HELLO deliberately carries no identifier of any kind: identity is
established by the Noise handshake, and nothing before it is trustworthy.

### 3.3 HANDSHAKE

Carries exactly one Noise handshake message, opaque at this layer. Its
sequencing is defined in §4.

### 3.4 GOSSIP_PUSH

Eager flood of full entries. `ttl` is the remaining hop budget: a relaying
node MUST decrement it and MUST NOT relay a message received with `ttl` of
0. Duplicate suppression (seen-entry tracking) is local policy, not wire
protocol.

### 3.5 Entries

An entry is the unit of replication: signed, append-only, content-addressed.

- **To-be-signed encoding**: `entry-tbs`, the deterministic CBOR of the
  4-element array (`author`, `sort-key`, `kind`, `payload`).
- **Signature**: Ed25519 (RFC 8032) by `author` over the `entry-tbs` bytes.
- **Entry id**: SHA-256 of the deterministic CBOR encoding of the full
  5-element `entry` array.

Identity and signature are always computed over the canonical
re-encoding of an entry's fields, never over raw received bytes, so a
non-canonical transmission cannot fork an entry's id. Receivers MUST
verify `sig` before storing or relaying an entry.

## 4. Session establishment

Sessions use the Noise Protocol Framework, pattern XX, suite
`Noise_XX_25519_ChaChaPoly_SHA256`: mutual authentication and forward
secrecy with neither party knowing the other's static key in advance. Each
peer's long-term identity at this layer is its Curve25519 static key; its
fingerprint is the SHA-256 of the static public key. Binding a fingerprint
to a person is an application-layer claim and is out of scope here.

This section is normative for protocol version 1. It is specified now and
implemented in a later milestone; until then implementations interoperate
in the cleartext epoch only.

### 4.1 Sequence

The link starts in the **cleartext epoch**. The party that opened the
transport connection is the initiator.

| # | Direction | Message   | Content                        |
|---|-----------|-----------|--------------------------------|
| 1 | I → R     | HELLO     | versions, features, transports |
| 2 | R → I     | HELLO     | versions, features, transports |
| 3 | I → R     | HANDSHAKE | Noise XX message 1: `e`        |
| 4 | R → I     | HANDSHAKE | Noise XX message 2: `e, ee, s, es` |
| 5 | I → R     | HANDSHAKE | Noise XX message 3: `s, se`    |

After message 5 both sides hold transport CipherStates and the link enters
the **transport epoch**: every subsequent outer frame body is a Noise
transport message whose plaintext is a padded inner message (§1). Cleartext
inner messages MUST NOT be sent after the handshake completes, and the only
messages permitted during the cleartext epoch are HELLO, HANDSHAKE, and
ERROR with code 2 (§5, §7).

### 4.2 Prologue

The Noise prologue is the concatenation of the two HELLO outer-frame
bodies, the initiator's first. Any tampering with the version and
capability exchange therefore breaks the handshake. The prologue contains
nothing else — in particular, no fixed protocol constant.

### 4.3 Rekeying

Static session keys for the life of a link are the failure mode to avoid.
Each direction MUST call the Noise `Rekey` function on its sending
CipherState after every 1 024 transport messages it has sent, and each
receiver does the same to its receiving CipherState on the same counts.
Counter-based rekeying needs no signaling. A session MUST NOT outlive 24
hours of continuous connection without a full new handshake.

### 4.4 Replay protection

Over the reliable ordered streams of protocol version 1, Noise nonces are
implicit and strictly increasing, so in-session replay is structurally
impossible; any decryption failure is unrecoverable and the link MUST be
closed silently (no error oracle; §7). For future unordered transports,
an explicit 8-octet nonce prefix and a 64-entry sliding receive window are
reserved; they are not used in version 1.

### 4.5 Handshake rate limiting

Handshake DH operations are the cheapest denial-of-service lever against a
node that anyone within radio range can reach. Implementations SHOULD
rate-limit handshakes per remote transport address (token bucket, burst 3,
refill 1 per 5 seconds) and SHOULD cap concurrent half-open handshakes
(32). These are local-policy defaults, not wire-visible values.

## 5. Version negotiation

Each HELLO lists every protocol version the sender can speak. The
effective version of the link is the highest version present in both lists,
computed by each side independently — a second round trip is unnecessary.

If the intersection is empty, a peer SHOULD send ERROR with code 2
(§7) and MUST close the link. This is the only ERROR permitted during the
cleartext epoch.

Both HELLO bodies are bound into the Noise prologue (§4.2), so a
version-downgrade attack surfaces as a handshake failure.

The wire protocol version increments only on breaking change. Additive
change — new optional map keys, new feature bits, new transport values —
rides on the extension points of §3.1 within a version. This document, spec
0.1.0, defines wire protocol version 1.

## 6. Set reconciliation

Reconciliation brings two peers' entry sets to convergence without either
transferring what the other already holds. Racnet uses range-based set
reconciliation as specified by Negentropy (Log Periodic); the algorithm
itself is not restated here.

### 6.1 Entry order and sort key

Negentropy requires a total order with a u64 sort key. Entries are ordered
by (`sort-key`, entry id): ascending `sort-key`, ties broken by ascending
bytewise comparison of the 32-octet entry id (§3.5). The `sort-key` is
milliseconds since the Unix epoch at entry creation, as claimed by the
author. It orders the set; nothing at this layer treats it as a verified
clock.

### 6.2 Session flow

A reconciliation session is opened by RECON_INIT, which carries a session
id `sid` and the sort-key window `[since, until)` the session covers
(`until` = `2^64 − 1` for "everything from `since`"). The `sid` MUST be
unique among the sender's open sessions on the link; either peer may open
sessions, and multiple sessions may be open concurrently.

Rounds proceed as RECON_MSG in alternating directions, each carrying one
Negentropy message. Either side ends the session with RECON_DONE — sent
when its Negentropy state reports reconciliation complete, or earlier to
abort (e.g. when a sync time budget expires). A RECON_MSG or RECON_DONE
whose `sid` matches no open session is a protocol violation (§7).
Reconciliation identifies the disjoint entry ids; the entries themselves
transfer via GOSSIP_PUSH or a future transfer type.

### 6.3 Negentropy payloads carried opaquely

The `msg` field of RECON_INIT and RECON_MSG is a Negentropy protocol
message, byte-for-byte as defined by the Negentropy specification, carried
as an opaque CBOR byte string. It is not re-encoded in CBOR: Negentropy's
own encoding is its conformance surface, and implementations are expected
to pass the upstream Negentropy test vectors unmodified (ADR-0007). The
Negentropy element id is the racnet entry id; the Negentropy timestamp is
the racnet `sort-key`.

## 7. Error handling

The ERROR payload is a single numeric code:

| Code | Meaning                                                      |
|------|--------------------------------------------------------------|
| 1    | protocol violation: malformed frame, padding, CBOR, or state |
| 2    | version incompatible (§5)                                    |
| 3    | message not permitted in the current link state              |
| 4    | resource limit exceeded                                      |
| 5    | internal error                                               |

ERROR is terminal: the sender MUST close the link immediately after
sending it, and a receiver of ERROR MUST treat the link as closed. Sending
ERROR at all is a SHOULD — a peer MAY close silently instead, which is the
correct behavior under active attack. There is deliberately no detail
string and no per-message error reporting: verbose errors are a metadata
leak and an oracle.

Two failure classes are always handled silently, never with ERROR: any
cryptographic failure (handshake or transport-message decryption or
authentication), and any failure during the cleartext epoch other than
empty version intersection. Codes 1 and 3–5 are only ever sent inside the
transport epoch, encrypted.

## 8. Conformance vectors

Any implementation must reproduce these byte sequences exactly. All hex is
lowercase, 16 octets per line. The vectors are generated from the fixed
Ed25519 test key whose seed is 32 × `0x01`:

```
seed       0101010101010101010101010101010101010101010101010101010101010101
public key 8a88e3dd7409f195fd52db2d3cba5d72ca6709bf1d94121bf3748801b40f6f5c
```

This key exists for test vectors only and MUST NOT be trusted by any
deployment.

### 8.1 Entry

The sample entry: `author` = the test public key, `sort-key` =
1 700 000 000 000 (`0x18bcfe56800`), `kind` = 0, `payload` = the 5 octets
`68656c6c6f`.

`entry-tbs` (51 octets) — `84` (array of 4), `5820` + author, `1b` +
sort-key, `00` kind, `45` + payload:

```
8458208a88e3dd7409f195fd52db2d3c
ba5d72ca6709bf1d94121bf3748801b4
0f6f5c1b0000018bcfe5680000456865
6c6c6f
```

Ed25519 signature over `entry-tbs` (64 octets):

```
81cd254612a44bc4fced2ab12fd17a8a
e5f29748da91ee7cf428fa5d43fab4a5
75b8eff21223cfe54215c76a05440b2b
4c7815ede94d0f328a6214a928a90008
```

Full `entry` encoding (117 octets) — `85` (array of 5), the four `entry-tbs`
fields, then `5840` + signature:

```
8558208a88e3dd7409f195fd52db2d3c
ba5d72ca6709bf1d94121bf3748801b4
0f6f5c1b0000018bcfe5680000456865
6c6c6f584081cd254612a44bc4fced2a
b12fd17a8ae5f29748da91ee7cf428fa
5d43fab4a575b8eff21223cfe54215c7
6a05440b2b4c7815ede94d0f328a6214
a928a90008
```

Entry id — SHA-256 of the encoding above:

```
9785dd7e1d47c2c71c38988f98d46f46
74dbf8dd7ed738284569ab6df2b08e8d
```

### 8.2 HELLO frame

`hello` with `versions` = `[1]`, `features` = 0, `transports` = `[1]`.
Breakdown: `0100` outer length 256 · `01` HELLO · `0009` payload length ·
`a300810101000281 01` payload `{0: [1], 1: 0, 2: [1]}` · zeros to 256.
Complete frame (258 octets):

```
0100010009a300810101000281010000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
0000
```

### 8.3 GOSSIP_PUSH frame

`gossip-push` carrying the §8.1 entry with `ttl` = 4. Breakdown: `0100`
outer length 256 · `10` GOSSIP_PUSH · `007a` payload length 122 · `a2 00
81` + entry + `01 04` payload `{0: [entry], 1: 4}` · zeros to 256.
Complete frame (258 octets):

```
010010007aa200818558208a88e3dd74
09f195fd52db2d3cba5d72ca6709bf1d
94121bf3748801b40f6f5c1b0000018b
cfe56800004568656c6c6f584081cd25
4612a44bc4fced2ab12fd17a8ae5f297
48da91ee7cf428fa5d43fab4a575b8ef
f21223cfe54215c76a05440b2b4c7815
ede94d0f328a6214a928a90008010400
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
0000
```

### 8.4 ERROR frame

`error` with code 2. Breakdown: `0100` · `70` ERROR · `0003` · `a10002`
payload `{0: 2}` · zeros to 256. Complete frame (258 octets):

```
0100700003a100020000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
0000
```

### 8.5 RECON_MSG frame

`recon-msg` with `sid` = 1 and `msg` = the 4 octets `00010203`. Breakdown:
`0100` · `21` RECON_MSG · `0009` · `a2000101 4400010203` payload
`{0: 1, 1: h'00010203'}` · zeros to 256. Complete frame (258 octets):

```
0100210009a200010144000102030000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
00000000000000000000000000000000
0000
```

### 8.6 Padding boundary

`handshake` whose `msg` is 249 octets of `0xaa` yields a payload of 253
octets and an unpadded inner length of exactly 256: no padding octets at
all. Breakdown: `0100` · `02` HANDSHAKE · `00fd` payload length 253 ·
`a10058f9` + 249 × `aa`. Complete frame (258 octets):

```
01000200fda10058f9aaaaaaaaaaaaaa
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
aaaa
```

With `msg` one octet longer (250 × `0xaa`), the unpadded inner length is
257 and the message pads to the next block: body 512 octets, frame 514.
