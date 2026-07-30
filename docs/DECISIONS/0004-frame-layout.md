# ADR-0004: Two-layer framing, u16 length prefix, no magic bytes

**Status:** accepted · **Date:** 2026-07-30

## Context

Frames travel over reliable ordered streams (BLE L2CAP CoC first). The
session layer (Noise, M3) will encrypt message contents, and padding must
sit inside the encryption boundary to be useful against traffic analysis.
The framing decided here is frozen by conformance vectors.

## Decision

Two layers (PROTOCOL.md §1). Outer frame: 2-octet big-endian length plus
body — nothing else. Inner message: 1-octet type, 2-octet big-endian
payload length, CBOR payload, zero padding to the block boundary. In the
cleartext epoch the outer body is the padded inner message; after the
handshake it is the Noise transport message of that same padded plaintext,
so introducing encryption changes neither layer.

Big-endian everywhere outside CBOR: network byte order, matching CBOR's
own integer encoding — one endianness rule in the whole spec. u16 lengths:
Noise caps messages at 65 535 octets, so wider fields buy nothing.

No magic constant and no outer version field. A fixed identifying constant
would make every frame trivially fingerprintable on the air, and would
embed project identity in wire bytes while the project name is an open
maintainer decision. Versioning rides in HELLO, which versions the framing
too.

## Consequences

- M3 inserts encryption between the two layers with zero framing change.
- 5 octets of overhead ahead of padding; padding dominates real overhead.
- Stream transports only; a future unordered transport needs its own
  adaptation layer (replay window reserved in PROTOCOL.md §4.4).

## Alternatives rejected

- **Varint lengths:** saves one octet on small frames that padding
  quantizes away anyway; costs parser complexity in untrusted input.
- **Magic prefix for resynchronization:** a reliable ordered stream never
  desynchronizes; the cost is permanent fingerprintability.
- **Single-layer framing:** would force M3 to either encrypt the padding
  boundary away or break the wire format.
