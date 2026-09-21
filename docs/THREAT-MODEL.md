# Android preview threat model

Status: engineering threat model, not an external audit or a security claim.
Scope: the current Rust core and Android public board, wire spec 0.2.0.
Foreground phone results are recorded in MEASUREMENTS.md; remaining hardware
acceptance is still pending. Stronger safety-critical use and public
launch remain blocked on the review and policy decisions described below.

## Assets and boundaries

- Local Noise and signing seeds establish persistent device/author identities.
  Android wraps their envelope with a Keystore AES-GCM key. An unreadable
  envelope stops startup instead of silently replacing the identity.
- Entries are signed, content-addressed, public append-only data. The app
  displays UTF-8 board posts; the core can retain other valid entry kinds.
- BLE/GATT/socket input and all peer messages are untrusted. The service UUID,
  PSM discovery, and BLE address are discovery/rate-limit inputs, not identity
  verification. The Rust session and entry parsers are the validation boundary.
- Android's private files directory is the local storage boundary. Messages
  have no separate application-layer at-rest encryption. OS/root/device
  compromise is outside the protection provided by the application.
- Debug APKs and ADB are trusted development facilities. The evidence runner
  requires authorized local ADB, uses `run-as`, and may capture visible posts.
  It must not be treated as a secure export mechanism for production data.

## Adversaries, controls, and remaining limits

| Threat | Implemented control | Remaining limit |
|---|---|---|
| Nearby observer | Noise encrypts established link frames | Fixed BLE service UUID reveals participation; traffic timing/volume remain observable |
| Active malicious peer | Strict framing/CBOR, entry signature checks, authenticated Noise transport, handshake admission limits | Anyone can create an identity and join; no person verification, invitation gate, or Sybil defense |
| Forged or modified posts | Ed25519 verification before insertion; canonical content IDs | A valid signature proves control of a key, not the author's real-world identity or truth of a post |
| Identity substitution | Noise authenticates the peer's static key within the handshake | No out-of-band verification or trust binding to a known person; signing and Noise keys are distinct |
| Replay and duplicate entries | Transport nonces and content-addressed deduplication | Valid old public entries can be retained/reintroduced; timestamps are author claims, not trusted time |
| Storage/memory exhaustion | Persistent journal limited to 64 MiB/10,000 entries; bounded link write queue; handshake/session limits | Attackers can fill storage or consume battery/CPU; no fair-share admission, quotas per author, or comprehensive availability guarantee |
| Interrupted writes/corrupt files | Flush before indexing, journal locking, incomplete-tail recovery, signature validation on reload | A complete corrupt record stops startup; no automatic repair or backup; filesystem/device compromise can remove data |
| Message interception after relay | Publicness disclosed in the UI | Every recipient can read/copy/relay posts; no end-to-end private messaging or remote deletion |
| OS kills/background limits | Foreground service, persisted entries, reconnect reconciliation | Foreground checks have passed on the recorded device pair; unplugged background behavior remains unvalidated and delivery is not guaranteed |
| Malicious display content | Plain text display, bounded UTF-8 board payload, no active web renderer | Offensive/deceptive text remains possible; the core accepts valid signed opaque entries even when the board hides their kind |
| Supply-chain/build compromise | Dependency lockfile, tests, conformance/interoperability checks, CI | Custom protocol/crypto code and build dependencies have not been independently audited |

## Evidence, not assurance

See ANDROID-PREVIEW.md for actual test results. Deterministic simulation,
property tests, parser fuzzing, Noise interoperability, JVM/native integration,
and emulator persistence tests reduce particular implementation risks. They do
not establish a general security guarantee or real-world radio reliability.
CI includes offline emulator smoke tests; physical-device evidence remains
separate in MEASUREMENTS.md.

## Release gates and unresolved decisions

The preview must not be marketed as audited, private messaging, anonymous, or
suitable for safety-critical communication. An external review of the custom
Noise implementation, protocol, resource controls, and Android integration is
still required before stronger claims. Key verification, abuse/content policy,
moderation/contributor posture, durable release signing and key recovery, and
store distribution require explicit maintainer decisions. Recording the
current behavior above does not decide those policies or approve public launch.
