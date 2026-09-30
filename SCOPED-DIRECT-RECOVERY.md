# Scoped Direct recovery — candidate, physical acceptance pending

## Status and scope
This branch builds on `bc18bf7` and converts the proof-of-concept encrypted-datagram copy into a per-write, per-peer guarded mechanism. The prior QA experiment's 30.13 FPS / zero added freeze result does **not** establish acceptance of this implementation. No candidate runtime was installed while the phone was locked. The accepted Device Match pilot remains unchanged on PC-02. Public installer publication and cold-boot/upgrade acceptance remain outstanding.

## Authority and routing
- `consume-launch` validates `connection_route` and stores the selected route on the authenticated stream ticket. Exact `direct`/`relay` values are accepted; an invalid value is rejected before consuming the launch token.
- Missing route on legacy consumers grants no recovery. Refresh/sibling ticket code retains the bound route and existing rental/authority lifetime. A route change still requires the existing explicit handoff; RouteTelemetry cannot mint/modify recovery permission.
- Host's already-authorized stream ticket generates a grant in owned-child IPC. Mic sidecar receives none. Old IPC without the new field deserializes with recovery disabled.
- Each actual video bind records the negotiated PT and SSRC; no global PT96..103 heuristic is used by this path. More than one simultaneous track binding disables copying rather than guessing.
- Before each eligible original write, the peer must be Connected with a selected Host/Srflx/Prflx UDP pair at both ends. Relay, TCP and unspecified pairs fail closed. Numeric remote endpoint and local/related port are matched again at the UDP boundary. No ICE selection or fallback policy is changed.
- Task-local scope covers only one awaited original video send through encryption/mux. It is not inherited by spawned retransmission/control tasks. Encrypted datagram target, local port, PT, SSRC, sequence, header bounds, grant and binding generation must all match; scope is single-use and expires after 50ms.

## Bandwidth/lifetime controls
- Activated by the existing authenticated RTCP path's NACK probe: at least 50 attempted originals, at least 3 first NACKs, 2–25% NACK/original ratio in the sampled window. This ratio is a feedback heuristic, **not a synchronized packet-loss fraction**; delayed NACKs may refer to a previous reporting window. Evaluate hysteresis/stability in live testing before production claims.
- Active interval is 15 seconds, with implementation cap 20 seconds. Extreme reported loss (>25% of sample) suspends recovery immediately. No fresh trigger means it expires; clean-path copies are not permanently forced.
- Host grant currently caps extra IP+UDP bytes at 2000 kbps and extra datagrams at 1200/s per transport. Token-bucket burst caps: 100ms allowance bounded to 1400–32000 bytes and 1–32 packets. These caps are not a reservation of actual network capacity and do not replace future egress/headroom policy.
- Original packet is sent first. Additional `try_send_to` never waits, never encrypts a second time, and failure does not change original-send success. A blocked copy pauses extra copies for one second; consumed budget is not refunded as an amplification loophole.
- Wire-byte accounting includes IP+UDP headers, excludes link framing. Logged copy bytes/counters are separate from original send counters.
- Track unbind/rebind/drop invalidates pending scopes; peer non-Connected suspends budget/generation, close or sender drop revokes it. Reconnect cannot revive a prior-generation scope. Hard grant lifetime is derived from the ticket using a monotonic deadline.
- This is integrated into shared Host/streamer source with no PC-02 or handset identifiers. Clients not using the authenticated native ticket path receive no grant. Coverage of legacy/browser/other native launch paths requires explicit integration, not assuming they are all supported.

## Packaging
- Default streamer build includes guarded capability, but no copies occur without runtime grant + negotiated binding + active loss window + budget.
- Unscoped `cloudgime-datagram-redundancy-qa` builds are rejected at compile time on this branch; archived `bc18bf7` preserves that experiment.
- `web-server.exe --direct-recovery-contract` and `streamer.exe --direct-recovery-contract` exit without starting IPC/listeners or touching display configuration.
- Both portable bundle and offline installer paths validate the matching contract using `tools/portable/verify-recovery-runtime.mjs`, including the existing prebuilt-bundle path. This is a capability/hash identity gate, not live-quality certification or a signature verification system.

## Validation completed (only new/changed areas)
22 distinct targeted tests passed, including:
- policy parsing/expiry/legacy IPC and invalid keys (3);
- authenticated ticket route binding, refresh, invalid route and revoke handling (3);
- packet scope/memory/budget/lifetime/peer policy tests (13 total, including one real local SDP/DTLS/SRTP/UDP peer test and extreme-loss deactivation);
- binary-contract packaging checks (3).
The two localhost peers are a protocol integration fixture, not an Android emulator. The actual negotiated PT in that test is 120 to avoid hard-coded PT assumptions. No full Device Match/audio/Relay suite was rerun.

Release build and paired contract passed. Current test binary manifest is in workspace `06 TEST/20261001-scoped-recovery/candidate-contract.json`:
- web-server SHA256 `b312fd02ad2254ad48ce19029c2a2325727363efbc51ae2228b67048e3a978ea`
- streamer SHA256 `429d5029dd7e25460378771c89a7cf95116f7414fb2c6c5a1cd117b495812ab7`

Physical Direct quality, loss-trigger transition/hysteresis, real NAT local-port resolution, budget impact at higher bitrate, and reconnect/teardown under real load are **not yet accepted** for this candidate. Do not label it production ready solely from tests or reuse the previous QA quality pass. Test only directly impacted paths next.

## Resume
Prepared runner: `06 TEST/20261001-scoped-recovery/run-targeted.mjs` with `CG_ANDROID_SERIAL=10.144.1.211:5555`. It requires an unlocked phone, PC-02 idle/maintenance, verified candidate hashes and creates backups. It changes only candidate web-server/streamer plus temporary capture-recovery Sunshine; VDD helper/config remain untouched. It restores all test binaries and verifies cleanup. Runner/deployer syntax passed; live deployment path is itself not yet exercised for this candidate.
