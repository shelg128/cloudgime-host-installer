# Deadline/dependency-aware recovery — offline research prototype

## Decision

**NOT ACCEPTED for live deployment or a sub-1% quality claim.** The controller and deterministic event simulator are implemented and tested, but the first locked evaluation does not beat the fixed-share comparator consistently. It avoids unnecessary copies on a clean path and backs off optional protection under queue pressure. Under isolated loss and long RTT it saves bytes but has more deadline misses than fixed copies. These tradeoffs are retained, not averaged away.

No production code imports these modules. No SDK, codec, RTP format, audio synchronization, route policy, or installed binary changed. No rental was created for this work.

## Implemented behavior

- Original offered send times and bytes are unchanged across policies. Extra traffic **does** delay original packets at the model's shared FIFO bottleneck; do not confuse unchanged submission time with unchanged wire departure time.
- Optional protection earns credit from original bytes: <=25% cumulative, <=1328-byte carry, <=2Mbps token rate with1328-byte burst. A single pending candidate is bounded and cancellable by acknowledged delivery, scope change, suspension, staleness, deadline, or observed RTT growth.
- Adaptive choices see only earlier feedback. No future packet-loss schedule, true link capacity, or realized arrival time is passed to the controller.
- NACK recovery of key/reference/unknown frames is preserved regardless of the frame's own deadline. Only explicitly classified nonreference leaf frames can suppress a futile NACK. This is a **synthetic-mode experiment**, not a safe production classification feature.
- The prototype estimates one-way delay from half RTT, assumes a110ms deadline from first Host send and14ms decoder service, and uses a small6ms optional dispatch delay. These are model parameters, not measured absolute timing or recommended production values.

## Workload and reproducibility

Evidence: workspace `06 TEST/20261006-deadline-recovery-offline`.
Historical synthetic-fixture packet header capture: `20261006-independent-sync-live/session-1791223858512/headers.pcapng`.
SHA256 `4e08367c00a6783a5e6ce16690c752eddfe24fe604c74d9d6bec0b87175a296b`.

-1,840 complete packet-bounded frames;22,205 unique originals;26,789,670IP+UDP bytes over60.693s (~3.531Mbps).
-36 repeat observations removed; first/last boundary frames excluded. Sender timestamps retained relative to first send.
-Header metadata cannot reveal an H264 reference graph. Simulator therefore uses declared synthetic GOP60 structures; it is **not a frame-accurate replay of the historical decoder**.
-Development84runs per model version; locked evaluation252runs (7scenarios ×6seeds ×3policies ×2metadata modes). Additional756sensitivity runs vary reorder grace0/8/20ms and all-reference versus sparse-reference synthetic graphs. These are correlated simulations of one workload, not physical experiments or statistically independent network sessions.
-`evaluation-lock.json` freezes controller/simulator/base test/protocol/workload hashes before held-out runs. Evaluation performed without retuning.26tests include byte accounting, determinism, original schedule, recovery preservation, scope/clock/revocation/staleness, results completeness, and explicit unfavorable outcomes.

Run tests from repository:

```powershell
$env:CG_DEADLINE_RECOVERY_EVIDENCE='C:/Users/Admin/Documents/CLOUDGIME WORKSPACE/06 TEST/20261006-deadline-recovery-offline'
node --test tools/qa/deadline-recovery/controller.test.mjs tools/qa/deadline-recovery/validation.test.mjs
```

Workload generation, evaluation, sensitivity, and summary scripts are retained under the evidence `scripts` directory. Runners refuse to overwrite completed result files. To reproduce fresh performance runs, use a new evidence directory and retain source/input hashes. Evidence checks intentionally fail if frozen evaluation code is modified.

## Locked evaluation: conservative metadata

Mean modeled deadline-miss percentage,6seeds each; **not W3C freeze%, not input-to-photon**:

| Scenario | NACK-only | Fixed-share cap25% | Deadline-aware |
|---|---:|---:|---:|
| Clean |0.000|0.000|0.000|
| Isolated loss |21.585|19.357|20.734|
| Delayed packets |20.389|18.243|19.194|
| Correlated burst |8.361|7.853|8.288|
| Capacity pressure |28.922|99.764|28.922|
| Capacity dips |13.813|14.746|13.832|
| Long RTT |26.748|23.623|26.014|

Fixed-share actually emits9.056% additional original-byte volume in this workload due to byte/packet sizes and small credit carry; it is a cap, not an asserted25% wire rate. Adaptive optional overhead ranges0–5.955% here. Fixed-copy pressure collapse is a model observation, **not a claim that the old physical25% candidate would exhibit99.764% freezes**.

Metadata-known synthetic experiments reduce retries for expired leaf frames but deliberately leave some such frames unavailable. Report these missing frames and do not equate bandwidth savings with preserved visual quality. All-reference sensitivity increases deadline misses and does not make the controller a consistent winner.

## Model correction, preserved evidence

V1 used no reorder grace. Bounded random jitter created premature gap feedback and optional traffic even on the nominal clean path.84development results and source snapshots are preserved in `v1-archived` and the original results file. V2 adds8ms common grace for all treatments, documented before evaluation. Controller thresholds were not tuned to held-out outcomes. Sensitivity at0/20ms retained; the8ms setting is **not a proposed change to the real WebRTC NACK timer**. The simulator is not calibrated well enough to predict physical freeze percentage.

## Integration blockers / model limitations

1. **Feedback:** per-packet positive ACKs are hypothetical, not established in the current Host API. Model charges78bytes each ACK/NACK and serializes them on a separate same-capacity reverse link, but assumes lossless feedback. Actual RTCP batching, congestion, authentication, event loss and ack mapping require a real adapter.
2. **Dependency metadata:** older captured High-profile Host bitstream has1IDR+179reference frames, no identified leaf frames from NAL header flags. This is not the current Baseline stream. `nal_ref_idc` is available in the Host H264 parsing/payloader code, but it is not a full dependency graph or alone a validated frame-drop certificate.
3. **Congestion integration:** reviewed `video.rs` track callback ignores REMB with a comment about lack of Moonlight dynamic bitrate support. Interceptors may handle other feedback; this local observation is not proof that all project congestion controls are absent. Adaptive encoder bitrate and total original+repair budget need end-to-end audit before claiming link capacity adaptation.
4. **Decoder/playout:** simple parent-completion model with fixed14ms decode; no hardware reorder, concealment, keyframe feedback, jitterbuffer implementation, audio, GPU, receiver scheduling or photon measurement. Deadline origin excludes capture/encoding/input delay.
5. **Network:** one full-duplex FIFO with synthetic delay/loss/capacity schedules. No production pacer, AQM/drop-tail queue, WiFi contention, cross traffic, audio traffic or real congestion controller. Synthetic packet-attempt random losses/time bursts are identical exogenous rules, not counterfactual replay of observed historical loss. Queue service responds to each policy's added traffic.
6. **Comparator:** fixed-share and NACK-only are models, not exact SDK implementations. No production packet-per-second token cap or exact grant/rate/revoke implementation is being certified by these results.

## Next engineering gate

Before another live experiment, build a real feedback/deadline adapter and conservatively classify encoder-produced access units on recorded baseline bitstreams. If nearly all frames are references, evaluate encoder-supported dependency layout/protection rather than pretending expendable frames exist. Verify quality with actual decoder replay. Re-evaluate candidates against both NACK-only and fixed protection with equal budgets and original load. Do not implement frame dropping, FEC, new ACK traffic, or a bitrate fallback in production merely because the simulator suggests it.

Current deliverable is a reproducible negative/mixed research result plus an isolated testbed, not a new production streaming algorithm.
