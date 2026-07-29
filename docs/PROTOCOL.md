# Racnet Wire Protocol

**Spec version: unreleased (pre-0.1)**

This document is the source of truth for the Racnet wire protocol. Where any
implementation disagrees with this document, the implementation is wrong.
Changes to this document are their own versioned commits and land before the
code that implements them.

Every section below is TBD, to be written in Milestone 1.

## 1. Framing

TBD in M1. Byte-exact field tables, endianness, length rules, padding to fixed
block sizes.

## 2. Message types

TBD in M1. Registry of message type identifiers, including a reserved
identifier for lazy-IHAVE announcements (future Plumtree forwarding layer).

## 3. Payload schemas

TBD in M1. CDDL definitions for all CBOR payloads.

## 4. Session establishment

TBD in M1. `Noise_XX_25519_ChaChaPoly_SHA256` handshake sequence, rekeying
policy, replay protection, handshake rate limiting.

## 5. Version negotiation

TBD in M1.

## 6. Set reconciliation

TBD in M1. RBSR (Negentropy-style) message set; entry ordering and u64
sort-key convention.

## 7. Error handling

TBD in M1.

## 8. Conformance vectors

TBD in M1. Fixed byte sequences that any conforming implementation must
reproduce exactly.
