# Video absolute-send-time isolation — QA trial rejected

## Hypothesis and evidence, not a root-cause claim
- Archived phone RTC event field17 reports AST; pcap video headers have incomplete extension blocks, so **do not guess AST IDs from truncated capture**.
-22226matched RTPseq+timestamp headers. Receiver AST aligns to first HostNICsend within median0.049ms/p950.148ms.
-388matched sequences have observed sends spanning>100ms. Their AST remains close to firstsend (median0.046ms), while latestsend-AST median181.96ms/p951638.33ms. A receiver records firstaccepted metadata; this does not prove which physical copy arrived.
-Existing Rust NACKresponder caches packetheaders, including originalAST. ReferenceWebRTC mainbranch refreshes AST at egress. The receiver bitrate estimator may use AST. **These facts do not establish that AST caused CloudGime freezes.**

## Safe isolation, not re-encryption patch
Feature `cloudgime-video-noast-qa` is defaultoff and implies the already scoped25%copy-share candidate. Omit optional AST from video negotiation/sending only; audioAST unchanged, video playoutdelay0/0 unchanged, codec/H264/NVENC/resolution/FPS/budgets unchanged. Never mutate already protected bytes or re-encrypt altered packets with a reused SRTP index; no SRTP/RTX/security changes.

## Validation
-72WebRTC tests pass for featureon; default and25% suites pass. Actual SDPoffer test confirms videoAST absence only when opted in; audioAST present and playoutdelay unchanged.
-Strict parser tests reject missing/truncated/overrunning extensionblocks. RTP payload is never extracted into evidence.
-First physical attempt1790917884726reached nativevideo but no full measurement: statusfile briefly absent during testfixture replace; billing settled/cleanup/rollback.
-Fixture fixed separately to atomicFile.Replace with retained.previous metadata; bounded100msx4IO retries and exactsession+3s freshness checks. Atomic/read-concurrency/identity/staleness tests pass. No media acceptance gate widened.
-One readiness retryfailed ICMP preflight, created no rental. Next run1790918956510completed96.702822s:3316kbpsmedian,29.6475FPS,24.712sfreeze=25.5546%,latencymedian168/p95299ms. **FAIL strictsub1 and latency/FPS gates.**
-Initial trial/videoSF stillDirectP2P; no routefallback or root cause attribution. Do not promote thisfeature or removeAST by default.

## Limits
Source reference is fetched upstreammain, not exact Android SDK build. No contemporaneous validA/B during this continuation; comparisons with historical2.6%passing windows are not causal. Manual5000kbps target further limits what REMBchanges could influence. Hypothesis not shown to fix quality; investigate further without claiming causality.
