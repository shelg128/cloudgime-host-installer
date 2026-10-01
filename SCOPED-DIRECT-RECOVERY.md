# Scoped Direct recovery — active-media pilot PASS, release acceptance limited

## Status and scope
This branch builds on `bc18bf7` and converts the proof-of-concept encrypted-datagram copy into a per-write, per-peer guarded mechanism. The prior QA experiment's 30.13 FPS / zero added freeze result does **not** establish acceptance of this implementation. The latest variant has its own physical result below; the prior QA pass is not inherited. All runtime experiments were rolled back to the accepted Device Match pilot. Public installer publication and cold-boot/upgrade acceptance remain outstanding.

## Authority and routing
- `consume-launch` validates `connection_route` and stores the selected route on the authenticated stream ticket. Exact `direct`/`relay` values are accepted; an invalid value is rejected before consuming the launch token.
- Missing route on legacy consumers grants no recovery. Refresh/sibling ticket code retains the bound route and existing rental/authority lifetime. A route change still requires the existing explicit handoff; RouteTelemetry cannot mint/modify recovery permission.
- Host's already-authorized stream ticket generates a grant in owned-child IPC. Mic sidecar receives none. Old IPC without the new field deserializes with recovery disabled.
- Each actual video bind records the negotiated PT and SSRC; no global PT96..103 heuristic is used by this path. More than one simultaneous track binding disables copying rather than guessing.
- Before each eligible original write, the peer must be Connected with a selected Host/Srflx/Prflx UDP pair at both ends. Relay, TCP and unspecified pairs fail closed. Numeric remote endpoint and local/related port are matched again at the UDP boundary. No ICE selection or fallback policy is changed.
- Task-local scope covers only one awaited original video send through encryption/mux. It is not inherited by spawned retransmission/control tasks. Encrypted datagram target, local port, PT, SSRC, sequence, header bounds, grant and binding generation must all match; scope is single-use and expires after 50ms.

## Bandwidth/lifetime controls
- Activated by the existing authenticated RTCP path's NACK probe: at least 50 attempted originals, at least 3 first NACKs, 2–25% NACK/original ratio in the sampled window. This ratio is a feedback heuristic, **not a synchronized packet-loss fraction**; delayed NACKs may refer to a previous reporting window. Entry uses that threshold. After entry, a fresh window with >=50 originals and any first NACK renews recovery (hysteresis); zero-NACK or missing feedback does not extend it. Extreme loss resets the policy. Peer disconnect resets both policy and budget generation.
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
22 original targeted tests passed, including:
- policy parsing/expiry/legacy IPC and invalid keys (3);
- authenticated ticket route binding, refresh, invalid route and revoke handling (3);
- packet scope/memory/budget/lifetime/peer policy tests (13 total, including one real local SDP/DTLS/SRTP/UDP peer test and extreme-loss deactivation);
- binary-contract packaging checks (3).
The two localhost peers are a protocol integration fixture, not an Android emulator. The actual negotiated PT in that test is 120 to avoid hard-coded PT assumptions. No full Device Match/audio/Relay suite was rerun.

Release build and paired contract passed. Current test binary manifest is in workspace `06 TEST/20261001-scoped-recovery/candidate-contract.json`:
- web-server SHA256 `b312fd02ad2254ad48ce19029c2a2325727363efbc51ae2228b67048e3a978ea`
- current hysteresis streamer SHA256 `762554362864841b12fb60bf0da01e9d879483c050e107652ecd50fb05950b4f` (replaces initial `429d5029...` candidate)

## Physical results — 1 October 2026
- First run `capture-resize-1790809263083` ended before measurement: backend reason STALE_HEARTBEAT. Sentinel runtime and billing monitor source matched the installed package. No synthetic heartbeat or stale timeout relaxation was used. Root cause of this one-off lease outage is still unresolved.
- Passive Sentinel diagnostics were tested with its integrity marker updated as a matching package pair, then both baseline ASAR and baseline hash marker were restored. Later original-package runs remained live through the test. Diagnostics are not a billing fix.
- Initial `1ac2b05` observation mixed capture preparation with measurement (zero video for ~20s), giving 26.8fps and misleadingly favorable freeze fraction. This is not final pacing acceptance. A subsequent active-media observation failed at 6.865s freeze / 177.599s (3.87%).
- Source/log review showed the entry threshold was reused for retention: protection reduced NACKs, expired every ~15s, then restarted after loss. Added six deterministic hysteresis tests; five directly affected recovery/peer tests also passed. Permission, byte/packet caps and timeout boundaries are unchanged.
- Latest evidence: `capture-resize-1790813127496`, 720x1570 Device Match @30fps, physical Android original normal APK, original Sentinel, capture-recovery Sunshine. After a separately recorded 7.552s readiness wait, 5456 frames / 181.351846s = **30.085fps**, six freezes totaling **3.264s (1.80%)**, zero added pause count/duration. One stable Direct srflx/UDP observation; no Relay fallback. PASS for unchanged 3% freeze threshold, not zero lag.
- NACK delta 91. RTT median 73ms, p95 327ms, maximum 517ms; receiver frame gap maximum 1225ms. These spikes remain limitations and no motion-to-photon/input latency claim is made.
- Sampled Host subwindow: extra 9,656,320 IP+UDP bytes / 174.423s (~**443kbps**), 9,053 copies, limited=0, blocked=0. Budget cap 2000kbps/1200pps not hit in this workload; higher bitrate and congested-path performance remain unverified.
- Log parser/readiness change is scoped to acceptance instrumentation. Startup evidence is retained separately, not silently discarded; delays and reconnects outside the active-media window remain product-level acceptance work.
- Passing Device Match/audio/Relay suites were not repeated. New code is integrated in universal source, not a PC-specific transport branch.

High-bitrate/headroom behavior, full startup/reconnect quality, other client launch paths, installer/cold boot and public native-stop remain outstanding. No broad production release or universal all-device claim is made.

## Resume
Prepared runner: `06 TEST/20261001-scoped-recovery/run-targeted.mjs` with `CG_ANDROID_SERIAL=10.144.1.211:5555`. It requires an unlocked phone, PC-02 idle/maintenance, verified candidate hashes and creates backups. It changes only candidate web-server/streamer plus temporary capture-recovery Sunshine; VDD helper/config remain untouched. It restores all test binaries and verifies cleanup. Runner/deployer and paired rollback were exercised. Use CG_SCOPED_OBSERVATION=1 for readiness evidence. Final audit: no unsettled fixture billing, no active PC-02 claim/media; original Sentinel and Device Match baseline restored.
