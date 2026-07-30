# ADR-0009: Threat-model posture — adversarial wire discipline, modest claims until audited

**Status:** accepted · **Date:** 2026-07-30

## Context

The project brief (§14) reserved the threat-model tier for the maintainer:
festival/convention convenience at one pole, protest/activist safety at the
other. The tier governs padding aggressiveness, metadata tolerance, whether
an external audit blocks launch, and what the README may claim.

The project's ambition is a substrate — decentralized infrastructure that
can stand in for internet services. Infrastructure does not choose its
users: if the substrate succeeds, some users will eventually depend on it
in adversarial settings whether invited or not (the brief's §13 concern).
Meanwhile, wire-level security properties are cheap to design in and nearly
impossible to retrofit once conformance vectors freeze the bytes, whereas
process commitments (audits, cover traffic) can be added deliberately later.

## Decision

A split posture, decided by the maintainer:

- **Wire discipline to the adversarial tier.** Everything frozen into the
  wire protocol is designed as if high-risk users exist: padding inside the
  encryption boundary, no fingerprintable constants, minimal metadata, no
  error oracles, silent closes on cryptographic failure. Spec v0.1 already
  conforms; future wire changes are held to the same bar.
- **Claims and audience at the modest tier until audited.** No marketing of
  safety or security before an external audit; the README states plainly
  what has and has not been reviewed, and that the project must not be
  relied on against a resourced adversary. An audit is a blocker for
  *security claims*, not for shipping.
- **Open to deliberate upgrades.** Expensive adversarial-tier additions
  (cover traffic, timing obfuscation, device-seizure story, Sybil
  resistance beyond the basics) are adopted as explicit, versioned
  decisions when a concrete need justifies their battery and bandwidth
  cost — not by default, and not silently.

## Consequences

- Milestone 3 implements the session layer to the spec's existing
  conservative rules; no padding-policy change is needed for it.
- README and any release notes carry the not-audited statement until an
  audit happens.
- High-cost protections are deferred without being designed out: the wire
  format leaves room for them, and adopting one later is an ADR plus, where
  wire-visible, a version bump.
- The open §14 threat-tier question is closed; the project name remains the
  only §14 item open with wire-adjacent effects, and it is kept out of
  wire-visible bytes regardless.

## Alternatives rejected

- **Convenience tier throughout:** cheaper now, but bakes weak wire
  properties into frozen bytes that real future users may depend on;
  retrofitting padding or metadata hygiene is a breaking change.
- **Full adversarial tier as launch posture:** makes an external audit and
  costly traffic-analysis defenses blockers for everything, front-loading
  costs the project cannot yet justify and delaying all field experience.
- **Configurable per-deployment tiers:** wire-visible knobs partition the
  anonymity set and let configuration choices fingerprint users; one wire
  discipline for everyone is safer than a menu.
