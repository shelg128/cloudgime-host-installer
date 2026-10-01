# Sub-1% target: QA variants rejected, prior 25% candidate preserved

## Target locked before testing
User requests freeze time <1%, ideally <0.5%, while preserving latency, resolution and FPS on the same Wi-Fi. `quality-gate.mjs` in workspace `06 TEST/20261002-subpercent` evaluates the full counter window and rejects source changes, FPS/latency/quality-load regression. No acceptance threshold was loosened.

Two previous 25% candidate runs defined the envelope before experiments: decoded FPS >=29.7131145, actual median video>=3286kbps, fixed H264 NVENC720x1570@30 and requested5000kbps. Per-run estimated latency median/p95 limits: gameplay105/181ms, buffer29/100ms, decoder44/50ms, processing79/150ms. These are telemetry proxies, not measured input-to-photon latency or proof of equal visual fidelity. Whole-window counters must remain valid, freeze<1%; <0.5% is separately recorded.

## Results — no candidate accepted
| Variant | Freeze | FPS | video median kbps | gameplay median/p95 ms | Status |
|---|---:|---:|---:|---:|---|
| Immediate copies,10% share |1.41685%|29.54787|3376|114/137|FAIL freeze, median latency and FPS gates|
| Immediate copies,5% share |3.54455%|29.85157|3373|100/155|FAIL freeze|
|25% budget,one delayed slot,3–15ms expiry|4.95392%|29.84760|3264|87/176|FAIL freeze/video-load and other latency gates|
|25% budget,one delayed slot,3–45ms expiry|4.14356%|29.70325|3368|98/130|FAIL freeze/FPS|

All were separate sequential physical runs with same phone, Wi-Fi, route, APK, encoder, resolution and requested bitrate. Network variability remains; single-run differences are not causal proof. No selective best-window reporting. All four passed the unchanged bandwidth ceilings and completed cleanup/rollback. No low-freeze candidate is accepted while latency/FPS regress.

15ms delayed slot scarcely dispatched copies (observed0.34448% of original bytes); most expired before the next30fps frame.45ms version allowed a single-frame crossing and reached8.38479% but still failed. No larger queue or unbounded timer is justified by these results.

## Code containment
- `cloudgime-recovery-share10-qa` and `cloudgime-recovery-share5-qa` opt in to lower share;25% branch and ordinary defaults unchanged.
- `cloudgime-recovery-delay-qa` is QA-only, absent from defaults. Original packet goes through normal send first. One <=1280-byte encrypted slot per peer budget is drained on later eligible originals, no background tasks/timers or blocking wait. Packet bytes are copied unchanged; no second encryption, control packet duplication, NACK-response changes or automatic routing fallback.
- Delayed scope requires exact selected endpoint/port, SSRC/PT, live binding, same generation, monotonic authority and bounded age. Suspension/backpressure clears slot/credit. Budget is spent on actual dispatch; copy share is reserved on enqueue and never refunded. Standard rate/pps ceilings remain conjunctive. Ordinary non-share budgets retain pre-existing immediate behavior.
- Existing real-socket tests continue with default path; additional socket/real-peer tests cover delayed flow, expiry, revoke, endpoint/SSRC/PT mismatch and cumulative limits.
- Final source preserves45ms QA version; previously tested15ms binary and hash are archived in its evidence folder.

The user target remains OPEN. Preserve a41da8f as the earlier candidate that passed the old3% gate twice, not as a sub1% pass. The live test binaries were rolled back after every experiment. Do not promote any of these QA flags into default or release them as a finished fix.
