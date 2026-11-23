# Rust safety and resilience audit

The 2026-11-11 audit reproduced three packet panics in the former transport
dependency. The server-owned transport retains the protections below. These
transport checks are independent of the game executable version.

## Fixed failures

| Original finding | Implemented protection |
| --- | --- |
| Short embedded piggyback packets asserted in `Packet::set_len`. | Validate the embedded header before creating a packet; limit nested piggyback processing to eight levels. |
| Fragment sequences outside their advertised range indexed outside a vector. | Validate sequence membership and maximum range before allocating; reject conflicting ranges for an existing assembly; use checked vector access. |
| A non-fragment reliable packet inside a fragment range caused a panic while reporting the mismatch. | Discard the incoherent bundle and continue receiving packets. |
| A batch of pending single acknowledgments could overfill an outgoing packet during garage synchronization. | Fit acknowledgments within the one-byte count and the remaining encrypted datagram budget, reserving space for later footers. Keep excess acknowledgments queued for subsequent packets. |
| Invalid Blowfish padding asserted or produced an invalid plaintext length. | Validate padding 1–8 and the resulting minimum header length before resizing. |
| Fragment ranges and peer/reliability maps could grow without limits. | Apply the limits below before retaining input; periodically reclaim expired state even without new traffic. |
| Synchronous RSA work and packet polling could monopolize the gateway. | Limit login attempts before decryption, bound RSA ciphertext, and yield from each poll at its deadline or work budget. |
| The toolkit's Blowfish union used unsafe references. | Replace it with byte-array XOR; preserve the wire transform and enforce `forbid(unsafe_code)` in the server targets. |

Also hardened oversized element length arithmetic, untrusted blob/collection
lengths, and pickle expansion. Pickle graph validation precedes recursive value
construction, bounding depth, instructions and expanded copies; compact shared
references cannot cause unbounded materialization. Network STRING/PYTHON codecs
use a smaller expansion budget and retain undecodable data as raw bytes, matching
their existing fallback contract. Server session accesses and ticket removal no
longer rely on unchecked indexing or `unwrap`.

## Operational bounds

Protocol constants live in [Mercury limits](../src/shared-infrastructure/src/mercury/net/limits.rs). These are
hard transport guardrails, independent of PostgreSQL and application admission.
They are currently compile-time limits, not extra configuration-file fields.

| Resource / work | Bound |
| --- | --- |
| Retained channel/off-channel states | 512 per protocol instance |
| One incoming bundle | At most 256 packets (each at most 1472 bytes) |
| Fragment assemblies | Four per channel; at most 4096 reserved fragment slots across the protocol |
| Buffered incoming packets | At most 4096 globally, 256 per reliable channel |
| Completed bundle queue | 16 per channel; remaining contiguous packets stay in the bounded reliable buffer |
| Incoming acknowledgement queue | 256 per channel |
| Reliable outgoing packets | 2048 per channel, 16384 globally; BaseApp/outbox return `WouldBlock` before sending when full |
| Incomplete fragments | Expire after ten seconds without progress |
| Inactive transport states | Expire after 120 seconds |
| Pending login/BaseApp handshakes | 256 each; expire after 30 seconds |
| Unacknowledged outgoing packets | Reclaimed within 30 seconds; BaseApp also runs its existing resend/give-up policy |
| Protocol input | 8192 packets per second, including embedded piggybacks |
| Application poll | At most 32 receive iterations and 256 elements per bundle; respect the supplied deadline |
| Login decoding / RSA | 32 attempts per second globally, eight per source IP; at most 2048 RSA ciphertext bytes per request |
| Blob / collection decoding | One MiB per blob; 4096 collection entries |
| Network pickle expansion | 4096 visits/nodes, 64 KiB copied byte/string/class contents, depth 64, bounded instructions |

Generic pickle decoding has a larger bounded budget for offline/cache consumers:
four MiB input, 131072 nodes/expanded visits, 16 MiB expanded byte contents, and
depth 64. Network decoding uses the smaller budget. Application `max_clients` and
`max_pending` must each be 1–256; invalid startup settings return a diagnostic.

Periodic maintenance runs in both LoginApp and BaseApp polling, rather than
waiting for another fragment from the same assembly. Existing admitted session
idle cleanup remains in place. At capacity, incoming packets/new states are
rejected and reliable sends apply backpressure; overload may require a client to
retry. These limits bound memory/work, not a guarantee of availability against a
saturated network or a measured production throughput claim.

## Regression evidence

`tests/transport_resilience.rs` covers former panic paths and subsequent
valid traffic, mismatched fragment ranges including mixed reliable packets, oversized ranges without allocation,
per-channel/global budgets, expiry and capacity recovery, reliable-send
backpressure, login-rate limiting, all 256 padding-byte values at three packet
sizes, nested/shared pickle expansion, and 5000 deterministically generated
packets followed by a valid ping. `tests/rsa_login.rs` covers wrong-key RSA
rejection followed by valid encrypted login.

During the 2026-11-21 production training session on server 0.15.0, return
traces recorded the garage reset and a subsequent `EnableEntities` for each
observed participant. The game gateway then panicked with `not enough
available data` at 23:42:55, 23:52:28 and 00:01:20 local time
(Europe/Paris). XMPP closed as the process stopped, and the local client
logged a refused connection before the third restart. The outgoing ACK
writer checked available space once before writing the whole queue. The
transport regression queues ACKs, prepares a multi-packet resource response,
encrypts and decodes each packet, verifies deferred ACK delivery and
checks a subsequent valid request.

The final release binary also stayed running after receiving the two former
pre-login crash packets over UDP, then accepted the existing account using the
installed client public key and returned Account/showGUI and the garage cache.
This used a synthetic peer, not visual game-client validation. The final check
connected directly to BaseApp on loopback because the configured public
`advertised_base` was unreachable from this Mac; that configuration was preserved.
Public NAT/port-forwarding reachability was not validated.

The PostgreSQL/UDP garage integration suite verifies that normal encrypted and
plain login, account isolation, garage synchronization, preferences and reconnect
still work with these bounds. This is not exhaustive fuzzing or a mathematical
proof that all future code and third-party dependencies are panic-free.
