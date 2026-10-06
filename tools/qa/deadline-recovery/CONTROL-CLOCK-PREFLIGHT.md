# Control-clock qualification after the second scoped pilot

## Outcome

One additional authorized pilot was attempted, with the corrected daemon-aware deployment and mandatory web startup receipt **before billing**. That gate passed. Native Direct, official post-billing sibling handoff, unchanged rental/root/claim/billing commit, mode verification, and scoped peer-context discovery succeeded.

The pilot stopped at `HOST_CLOCK_SAMPLE_TOO_WIDE` **before visual review and before a capture request**. Its clock helper started a fresh SSH/PowerShell command inside an existing rental and rejected a round trip over 1500 ms. This tool precondition should have been qualified before billing. No second rental or capture retry followed the failure.

Session `ca146b57-92d5-486c-9266-2cdcae55e51f` ended and settled. Host binaries, APK, service environment and daemon state were restored. The failed pilot contains no Host capture. The successful idle clock experiments below must not be substituted for missing pilot data.

## Implemented correction: an already-ready, read-only clock exchange

`ClockProbe.cs` is a small native .NET console helper, compiled with the installed Windows Framework compiler. It accepts only precisely formatted `sample` or `exit` requests with a 32-hex nonce and sequential indices. It cannot execute arbitrary commands, read files, or mutate services/billing/media.

Bounds:
- maximum input line 200 characters;
- maximum eight samples, one explicit exit;
- 3-second input timeout;
- independent 20-second process lifetime;
- automatic stdout flush;
- invariant-culture integer/floating-point JSON serialization;
- no secret, credential or key persistence.

`exchange-control-clock.mjs` starts a caller-pinned helper directly or through caller-pinned SSH. It waits for its ready message before timing the exchange. It submits all samples and exit in **one bounded batch**, then verifies nonce/order/count, response format, output bound, close acknowledgement and process exit. Any malformed/extra output, partial result, bad scope or failed exit rejects the entire exchange. It does not retry a rental or issue capture commands.

One batch is used because repeated interactive writes over the observed Windows SSH invocation were not reliably delivered: attempts stopped with `CLOCK_INPUT_TIMEOUT`. This is an observed limitation of this invocation path, not a proven defect of all SSH or PowerShell configurations. The same protocol worked locally; batch delivery worked on PC-02. Six replies in a batch are **correlated**, not six independent network samples.

## Offset bounds, not media latency

`control-clock.mjs` uses four timestamps:
- `L0`: PC submission of the batch;
- `H1`: Host reads the corresponding request;
- `H2`: Host constructs the reply;
- `L3`: PC receives that reply.

With nonnegative transport/processing delay and approximately stable clocks, the Host-minus-PC offset is conservatively bounded by:

```
lower = H2 - L3 - 2 ms
upper = H1 - L0 + 2 ms
```

The 2 ms allowances account for integer timestamp quantization; they are not a certification of Windows clock accuracy. All messages share the same conservative batch send time. We select the narrowest individually valid interval only if **every** sample passes clock/order validation and the intervals overlap. A single successful sample in a failed session cannot qualify.

The unchanged width ceiling is 1500 ms. Tests compare local wall time with local monotonic elapsed time and Host wall/monotonic processing duration; inconsistent offsets or detectable clock steps reject the measurement. Drift expansion uses an explicit assumed maximum of 100 ppm and a maximum mapping lifetime of 180 seconds. This is not a measured oscillator guarantee. A subsequent OS wall-clock adjustment can invalidate an earlier mapping; refresh near use and reject wall/monotonic divergence. **Do not use this mapping for capture-to-photon, audio/video alignment, or one-way media latency.**

`mapEvidenceLowerBound` maps the **original** observation time, not current time. Older evidence therefore remains old; drift uncertainty only makes the lower bound more conservative.

`requirePrebillingClock` requires a complete successful exchange, clean remote exit and execution freshness within 30 seconds. It returns a tool-preflight result, not rental authorization. A timing qualification before billing is not a license to reuse its offset indefinitely during login: obtain a new mapping near capture, using the qualified method.

## Actual verification without another rental

- Native local pipe: six valid samples, close acknowledgement, exit 0.
- Actual SSH to idle PC-02: six valid samples, close acknowledgement, exit 0.
- First successful remote batch: interval width **17 ms**; independent final helper invocation: **20 ms**. SSH startup was about 646/629 ms in those runs and was excluded from the post-ready exchange interval.
- These figures are control-clock offset uncertainty, **not** video latency.
- Six local protocol cases passed: complete batch, nonce mismatch, sequence mismatch, oversized request, sample cap, EOF.
- Leaving stdin open without input caused `CLOCK_INPUT_TIMEOUT`, exit 2, at about 3.09 seconds. No hung remote clock process remained at final audit.
- All QA suites: 128 passed, 0 failed, 0 skipped, including retained failed-pilot evidence and previous prototype/adapter/observer qualification.

The helper was copied only into the existing admin/SYSTEM-protected QA directory on PC-02. Its final SHA256 was `a0ee82dc91e65c7698461b3f26d5aafee81ab6355530547e50df486f940497a0`. It was not installed as a service or in a production executable path. SSH host-key checking and the existing authorized host key were retained; credentials are not stored in source.

## Preserved failures

1. The live pilot failed before capture at the cold-command clock-width gate. That measurement's actual width was not saved before the throw; do not invent a number beyond the gate condition.
2. Read-only PowerShell helper attempts produced one or zero samples then timed out on input. They exited and remain archived.
3. Native compilation initially passed an absolute slash path to the Windows compiler incorrectly; corrected to a relative source/output with explicit working directory.
4. .NET Framework composite formatting of the final `:R` field produced literal `R` and invalid JSON. Raw response evidence is retained. Explicit `ToString("R", InvariantCulture)` serialization fixed it.
5. Native interactive remote exchange still timed out after the first sample; batch exchange resolved this tested invocation without increasing timeouts or accepting partial data.

No failed trial was relabelled as successful. The batch helper is a control-tool correction, not a new streaming algorithm.

## What has and has not been exercised

**Exercised:** daemon activation gate in a real billed pilot; complete post-billing native context; mode and progress; corrected clock-only helper locally and on idle PC-02; strict mathematical/process gates; full cleanup and settlement.

**Not exercised:** successful atomic capture-gate invocation in a real session, visual-review-to-capture latency, request expiry under live scheduling, current-stream frame trace completeness or any latency/freeze improvement. The new clock helper/prebilling gate is reusable source plus verified idle runner; it is **not yet wired into an executed live pilot**. Do not rerun the old failed runner unchanged.

Before a future pilot, stage and verify the helper; run `exchangeControlClock` and `requirePrebillingClock` before claim/billing; qualify the downstream command path with a non-mutating dry-run while idle; after stream/fixture readiness refresh the mapping and retain original evidence timestamps. No accepting expired screenshots or replacing clock uncertainty with a point offset. One rental maximum per explicitly bounded protocol; failures stop and settle rather than create replacement billing.

## Evidence and cleanup

Workspace `06 TEST/20261006-scoped-pilot-v2` and `20261006-scoped-pilot-v2-run/session-1791303333910` contain original failed execution, activation receipt, context, all clock attempts, final verification, protocol tests and audits.

Final audit: session ENDED/BILLING_SETTLED, cleanup COMPLETED, PC-02 maintenance without owner/claim. Web/streamer/Sunshine/config/APK baseline hashes restored; service environment equals backup; zero QA environment variables and zero `ClockProbe` processes. Trace capture count 0. HP unlocked, display ON, foreground lobby `mHoldScreenWindow` active, wireless ADB heartbeat fresh. No second rental after the pilot failed.
