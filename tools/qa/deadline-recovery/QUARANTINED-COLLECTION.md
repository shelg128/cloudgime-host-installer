# Explicit quarantined collection and actual-review/startup findings

## Result of this continuation

Actual assistant image review was measured, not simulated: image capture began at
1791325262082, verified at1791325263666, actual review was recorded at1791325310561.
Image age48,479ms and review-after-transfer46,895ms. The prior five-second
pre-capture review rule correctly rejected it. The one-second simulated review in
previous dry runs was never proof the chat tool cycle could meet that bound.

The protocol was therefore changed **explicitly**, not by backdating or pretending
review had passed. `quarantined-metadata-v1` permits collection of bounded diagnostic
metadata after fresh authorized scope/fixture/image checks, with
`visualApproved=false`, `analysisDisposition=quarantine-pending-review`.
Backend3s, original image5s, SDKprogress1s, fixture3s, exact process/hash/rental,
one-shot nonce, authority expiry,25s maximum and metadata quota remain enforced.
Legacy protocol behavior is unchanged. No route fallback or production recovery
algorithm was introduced.

Review is moved to **acceptance of immutable collected evidence**, not eliminated.
The intended bundle seals trace, pre/post image hashes, both capture nonces, request
nonce and context. Missing review, mismatch/tampering, incomplete trace, unverified
scope or continuity keep it quarantined. Even accepted review only permits
`reviewed-diagnostic-only`, never streaming-quality acceptance. File assertions are
not independent cryptographic backend authority; callers must supply verified
scope/continuity evidence. Pre/post images do not prove uninterrupted rendering.

## One live attempt; no billed rental

The newly versioned pilot passed startup receipt and actual prebilling timing dry
run, then opened exactly one claim. It failed **before Sentinel login/billing** and
before scoped metadata collection, with `ANDROID_NATIVE_VIDEO_NOT_PROVEN`.
Claim cancellation returnedHTTP200. Direct database read by that exact claim found
**zero SessionRentalSession records**; PC02 has no owner or active claim afterward.
Do not call this a settled billed rental: none was created. No second claim was
attempted. All temporarily deployed binaries/APK/environment were restored.

The new live quarantine branch was therefore **not reached/proven in a real
capture**. Local tests cover its policy; the earlier pre-reviewed prebilling dry
run still ran for tool readiness. No trace was sealed/released or used to claim a
performance improvement.

## Concrete startup diagnosis (Host + physical Android)

- Native ICE CONNECTED/COMPLETED with public srflxUDP pair; RTT telemetry around
58–95ms. Data channels opened. Direct was not silently changed to relay.
- Moonlight upstream initialization and audio reception completed; Host forwarded
>2000audio RTP samples to WebRTC.
- Android23counter observations over79.353s show0video packets,0receivedframes,
0decodedframes. Track attachment is not rendering proof.
- Host: `Terminating connection due to lack of video traffic` and
`No video traffic was ever received from the host!`; stream error`-100` about10s
after Moonlight stream establishment.
- Thus failure is upstream of WebRTC video recovery: Sunshine/capture/encoder or
Sunshine→Moonlight local transport remains unresolved. Do not blame NAT/decoder
or compute a freeze ratio from zero frames.
- Exact Host startup line for this native root:
`RUNTIME_PROFILE ... runtime=default encoder=software`.
- Native requested660×1440@30/2500kbps, whereas three previously successful startup
runs requested720×1570@30 and rendered a first frame. This is a real fixed-profile
mismatch, not a matched performance comparison.
- The current Android source applies the1440/800display cap when the payload says
software. The Host runtime line directly proves selected metadata at failure,
but does not prove which encoder actually emitted video (none received).
- After rollback, profile and config areNVENC/DDX. Profile modification time is
22:32:41Z, **after** the failed stream. It cannot retrospectively certifyNVENC at
22:30:44Z. Sunshine's session log was overwritten by restart; its exact encoder/
capture error at failure is not available. Root cause of software selection and
zero source video is **not yet proven**.

## Added safeguards, not an encoder fix

`encoder-preflight.mjs` rejects fixed-profile QA when profile/config encoder or
capture disagree, or when evidence is stale/Host not idle. Capability metadata
is not hardware liveness. Minimal read-only target check after rollback passed
NVENC/DDX with zero media and maintenance slot. The guard was not present before
this failed attempt; do not misstate the chronology.

`PreRollbackEvidence.ps1` preserves bounded Sunshine/streamer/supervisor log suffixes
and relevant capability fields before future rollback. Embedded into the source
QA deployment script (not retroactively applied to the executed pilot).
Best-effort recording errors do not block safety rollback. Current idle snapshot
test captured four bounded files in655ms with serviceRunning. It **does not recover
historical overwritten logs**. Secret redaction is best effort, not complete;
artifacts stay in the admin-protected QA directory. No raw network payload recorded.

An unlaunched `execute-next-review-required.mjs` template in the evidence folder
adds profile preclaim checks and refuses automatic repeat without an explicit
source-video failure review. Existing executed runner artifacts remain unchanged.
No production codec/bitrate/decoder buffer/firewall/router settings changed.

## Tests and evidence

Legacy17PowerShell gate cases passed;7explicitquarantine gate cases passed;
169NodeQA tests passed with historical and current evidence, zero skips/failures.
Additional no-network acceptance tests verify that late review can only release
an exact immutable diagnostic bundle and cannot forge fresh collection permission.

Evidence roots:
- `06 TEST/20261007-actual-review-cycle`: actual JPEG, nonce/hash, true review timestamp,
48.479s failure; no backdated approval.
- `06 TEST/20261007-quarantined-pilot[-run]`: startup/billing chronology, logs,
exact-claim cancellation audit, encoder profile comparison, current snapshot test,
protocol and unlaunched-next-run template.

Generic prior audit expected one billed session and returnedfalse for this
prebilling cancellation. Kept unchanged; replacement audit queries sessions by
claim and validates zero records. No backend record was edited to fit the audit.

## Device/cleanup caveat

Phone stayed unlocked/on with30min screen timeout and wirelessADB active. Chrome
CustomTab periodically took foreground, leaving `mHoldScreenWindow=null`; this was
observed directly, not called a crash. Lobby was initially restored but the agent
did not close a user's browser to fake permanent foreground. A separate bounded
15min wake-only guardian (20sKEYCODE_WAKEUP,noPIN/tap/foreground/settings actions)
was started after testing; it is not media traffic and expires automatically.
Final audit must report whichever foreground/hold-screen state actually exists,
and must not claim indefinite keep-awake after the guard expires.

Next work should reproduce the **source-video/encoder-profile failure** with
pre-rollback logs and fixed encoder geometry, not create more packet-recovery
variants or repeatedly open paid sessions without new evidence.
