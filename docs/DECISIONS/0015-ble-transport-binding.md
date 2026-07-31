# ADR-0015: BLE L2CAP CoC transport binding

**Status:** accepted · **Date:** 2026-07-31

## Context

The wire protocol assumes a reliable ordered byte stream and deliberately
scopes the transport out. M4 needs the missing piece for BLE: how peers
find each other, learn where to connect, and open the stream — and iOS
(M5) must implement the identical procedure, so it has to be normative
spec text (PROTOCOL.md §9.1), not Android implementation detail. Three
choices in that binding deserve a recorded rationale.

## Decision

**A fixed random 128-bit service UUID is the entire advertisement.** This
is a knowing, scoped exception to ADR-0009's posture that keeps
identifying constants off the air (the frame layer has no magic bytes for
exactly this reason). Discovery between strangers requires a stable
beacon: with no shared secret and no synchronized rendezvous there is
nothing to rotate a private beacon against, and every comparable system
(bitchat included) ships a fixed UUID. The exception is bounded to
advertising data — frame bytes stay constant-free — and the upgrade path
(epoch-rotated UUIDs derived from community keys, for closed meshes that
do share a secret) is recorded as a future versioned change to §9.1. The
UUID is random, not name-derived: a UUIDv5 of the project name would let
anyone confirm the app from the name while buying nothing, since any
fixed value is equally fingerprintable.

**The PSM travels over GATT, not in the advertisement.** Android assigns
L2CAP PSMs dynamically per listening socket, so the value cannot be
baked into anything static — and decisively, CoreBluetooth cannot put
arbitrary service data into an iOS advertisement at all, while a GATT
characteristic read is the one mechanism both platforms support (it is
also Apple's documented pattern for `publishL2CAPChannel`). Cost: one
short-lived GATT connection before the channel opens; the diagnostics
screen measures it rather than guessing.

**The channel is the insecure L2CAP variant** (Security Mode 1 Level 1).
Every byte on it is already inside the Noise session's mutual
authentication and AEAD; requesting BLE pairing would add user-visible
pairing ceremony on every first contact and introduce a second trust
root — one anchored in BLE's association models, which are weaker than
and redundant with the §4 handshake.

**Duplicate links resolve by fingerprint tiebreak.** Dual-role peers can
open two crossed channels; HELLO carries no identifier, so dedup is only
possible post-handshake. Both sides keep the link whose initiator has the
lexicographically smaller fingerprint and silently close the other — a
rule both ends can evaluate independently on identical inputs, so no
coordination message (and no new wire surface) is needed. Connect jitter
of 0–2 s makes the crossed case rare; the tiebreak makes it harmless.

## Consequences

- Racnet presence is trivially detectable by radio scan. True of every
  discoverable BLE mesh; now it is written down rather than implied, and
  the threat-model document (pre-launch) must say it plainly.
- The GATT read adds ~100–300 ms (to be measured, not asserted) to every
  connection establishment.
- No BLE bonding state exists anywhere — reinstalls and address rotation
  cost nothing, and no platform pairing dialogs appear.
- The tiebreak burns one handshake's work when links cross; accepted, as
  crossed links are jitter-rare and the loser closes silently.
- iOS M5 inherits a binding it can implement without new spec work.

## Alternatives rejected

- **PSM in advertisement service data:** impossible to emit from iOS;
  also couples advertising payload to socket lifetime on Android.
- **Fixed/spec-assigned PSM:** platforms do not offer PSM choice for
  dynamic channels; SIG-assigned PSMs are for standardized profiles.
- **Name-derived (UUIDv5) service UUID:** confirms the project name to
  anyone who guesses it; no benefit over a random constant.
- **Rotating advertisement UUIDs in v1:** nothing shared exists to derive
  the rotation from; strangers could never find each other.
- **Secure L2CAP (pairing/bonding):** redundant trust root below Noise,
  user-visible ceremony, bonding state to manage — for no added security
  against the actual threat model.
- **Keeping both crossed links:** doubles per-pair radio and crypto cost
  and makes sync sessions ambiguous to reason about for no benefit.
