# ADR-0026: The local hub: a provisioned roster, replication in the runtime, WebSocket over mutual TLS in `keel-net`, a hub daemon and a provisioning tool

- **Status:** Proposed
- **Date:** 2026-11-09

## Context

Slice 5 of step 7 is the local hub ([ADR-0023](./0023-register-shell.md), decision 9):
"`keel-net`, WebSocket over TLS 1.3 with mutual TLS, the hub role in the app and in a Linux
daemon, and a provisioning tool standing in for the Device CA". It comes before slice 4, the
Android app: this environment can't reach Google's repositories, so the app can only be built in
CI, while all of this slice can be built and verified here, and slice 4 then hosts what it
builds.

ADR-0023 fixes the outline:

- The transport is Rust, written once for every platform and the daemon: WebSocket over TLS 1.3
  with mutual TLS, carrying `keel-sync`'s frames on one port; the hub listens and devices
  connect; on `tokio`, with `rustls` (one crypto provider for everything) and
  `tokio-tungstenite`. Certificates are pinned to the location's CA, and the hub's carries a
  stable name, so a new address doesn't break it.
- The hub role is hosted by the same runtime: in the app as a foreground service, and in a Linux
  daemon.
- Discovery: the provisioned address first; mDNS on Android later, through its `NsdManager`.
- A provisioning tool stands in for the Device CA: it makes a location's profile, enrolls devices
  (their keys into the location's registry) and issues their certificates from a development CA.

What is built shapes the rest:

- `keel-sync`'s replicator is sans I/O: the caller hands it each frame from a peer, named by the
  peer's device identifier, and a tick when it asks, and sends the frames it returns. It runs
  the election, the hub's answers and its sequencing through a `Replica`, which `StoreReplica`
  adapts from a store and a `DeviceRegistry` of the location's devices and their keys.
- The runtime owns its device's store, with one writer: replication must run through it, on its
  thread, not beside it.
- Nothing yet gives a device the registry: the simulator builds one in memory.

## Decision

1. **The roster v0**: the location's devices, in one document the provisioning tool writes and
   every device and the hub is provisioned with, as the profile is (ADR-0023, decision 3):
   - the location;
   - each device: its identifier, name, signing key (algorithm and public key), and priority as
     hub (none for a register that never hosts it);
   - the hub's address and port, and the name its certificate carries.

   Canonical CBOR, decoded strictly, with a format version; checked whole (identifiers and keys
   unique, at most 1,000 devices). Until the cloud signs rosters (step 8), it is trusted as the
   profile is: as provisioned.

2. **Replication in the runtime**, sans I/O, as `keel-sync` is: the runtime builds its device
   registry from the roster and starts a replicator over its store.
   - `start_sync(roster, now)`, `on_frame(from, bytes)`, `on_tick()` and `next_tick()`, each
     answering with the frames to send; and after every intent's write, the frames that push what
     it wrote, so a sale reaches the hub at once.
   - The runtime reports what the shell shows of it: whether the hub is reachable, whether this
     device is the hub, and whether its log is settled. Until it is settled, intents are refused,
     `RuntimeError::NotSettled`, as `keel-sync` requires.
   - The hub daemon is the same runtime with no one signed in.

3. **`keel-net`**, a platform crate on `tokio`, which moves frames between the network and a
   runtime and decides nothing:
   - one TLS 1.3 connection per peer, a WebSocket over it, each binary message one frame of at
     most `keel-sync`'s `MAX_FRAME` bytes; anything else closes the connection;
   - the hub listens on one port; a device connects to the roster's address, again after a backoff
     from 1 s doubling to 30 s when it drops;
   - mutual TLS: each side presents its certificate and verifies the other's against the
     location's CA alone; a peer is the device the identifier in its certificate names
     (`urn:keel:device:<uuid>`), and must be in the roster, or the connection closes; the hub's
     certificate carries the roster's stable name, which the device checks instead of an address;
   - `rustls` 0.23 with the `ring` provider, TLS 1.3 only: `ring` builds with a C compiler alone
     for every target, Android's included, where `aws-lc-rs` needs CMake and more;
   - its interface is two channels: frames in, each with the peer it came from, and frames out,
     each with its peer, and the peers connected now; the runtime's thread drives the runtime with
     them, so the store keeps one writer.

4. **The hub daemon**, `keel-hub`, a binary: a runtime over its own store, its roster and
   certificates from files, `keel-net`'s listener, and the loop that joins them.

5. **The provisioning tool**, `keel-provision`, a binary for development and pilots: it makes a
   location's development CA, and for each device a signing key, a TLS key and certificate, and
   the roster, all as files a device is provisioned with. It uses `rcgen` for the certificates. The
   cloud's enrollment (step 8) replaces it, with the same artifacts.

6. **Verification:**
   - known answers: the roster's encoding pinned against Python's `cbor2`, and every rule it is
     checked against; the certificate's identifier read back; a sale on one device reaching the
     hub and another device over real TLS on the host; a certificate from another CA, a device
     not in the roster, and a hub certificate of another name each refused; a device reconnecting
     after the hub restarts, and catching up;
   - the runtime's replication against `keel-sync`'s rules, by running the simulator's checks on
     runtimes: devices that ring sales through intents, a hub and a standby, under the simulator's
     faults;
   - planted bugs in the roster's checks and the peer's identity, caught by the tests.

## Consequences

**Positive**
- Devices and the hub sync over the network with the same runtime, and the same tests, on
  every platform.
- A peer is who its certificate says, and only a device of the location's roster.
- The slice is built and verified here, before the Android app, which only hosts it.

**Negative**
- A roster trusted as provisioned, and a development CA, until the cloud.
- `tokio` and `rustls` in the runtime's process: a larger binary, and a second crypto library
  beside SQLCipher's OpenSSL.
- One hub address, provisioned, until discovery.

## Alternatives considered

| Alternative | Why not chosen |
|---|---|
| Replication beside the runtime, with its own store handle | Two writers of one store, or a lock across both, where the runtime already serializes its writes. |
| `aws-lc-rs` as the provider | Faster, and FIPS-capable later, but its build needs CMake (and NASM on some targets), which Android's toolchain adds pain to; `rustls` can move to it with a feature flag. |
| A peer's identity by its address, or by a token | Addresses change on a LAN; certificates are what the Device CA will issue anyway. |
| Plain TCP with our own framing | WebSocket passes through the proxies and firewalls some stores have, and browsers will speak it later. |

## References
- [ADR-0019](./0019-replication-and-deterministic-simulation.md) to
  [ADR-0022](./0022-hub-election-and-failover.md); [ADR-0023](./0023-register-shell.md),
  decision 9; [offline-and-sync §3.3](../architecture/offline-and-sync.md#33-transport-discovery-and-security);
  [security §2](../architecture/security.md#2-identity)
- [rustls](https://github.com/rustls/rustls) 0.23, [tokio](https://tokio.rs) 1.54,
  [tokio-tungstenite](https://github.com/snapview/tokio-tungstenite) 0.30,
  [rcgen](https://github.com/rustls/rcgen) 0.14
