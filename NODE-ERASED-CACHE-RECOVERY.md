# Erased Node cache recovery and durable metadata writes

## Observed defect
On PC-02, `host_activation_state.json` contained 2,172 zero bytes after an intervening boot. The canonical manifest, machine identity and component credentials remained intact and authenticated successfully. The supervisor classified the invalid disposable cache as `node_binding_unavailable`, never reached revalidation and stopped runtime. Operator recovery previously archived the corrupt cache; normal hydration/revalidation restored the host without license setup or token rotation.

## Source candidate
This change recognizes only **erased** caches (empty/missing, all-zero, whitespace/BOM-only, <=64KiB). It starts from an unauthenticated default state, invalidates any in-memory authorization lease, then still requires canonical manifest+DPAPI identity+host credential consistency and a new server status verification before runtime is allowed.

It does NOT discard arbitrary invalid JSON, mixed NUL/text, oversized documents, access errors, valid owner conflicts, revocation or suspension. Valid cached fields are preserved for the existing strict binding checks. No new identity/license is created and no credential file is rewritten. Erased metadata is not authorization.

Runtime cache writes now:
1. create a unique temporary file exclusively;
2. restrict ACLs before payload bytes are written;
3. write and `sync_all` the complete content;
4. atomically replace via Windows MoveFileEx(REPLACE_EXISTING|WRITE_THROUGH), or rename+directory sync off Windows;
5. preserve the destination if an earlier step fails and remove only the owned failed temp file.

This improves durability but **does not prove the original zeroing was caused by a specific power/storage failure**. No physical power-cut test was run.

## Local validation
- 58 supervisor tests passed, including 6 new erased-cache/parser tests and 3 actual filesystem durable-write tests; existing identity/revocation/DPAPI tests passed.
- 3 source integration checks passed: lease invalidation/hydration, unchanged strict binding/server validation, ACL+flush ordering.
- Release host_supervisor built and copied as a separate runtime-agent candidate.
- Not installed on PC-02; baseline runtime and credential hashes remain unchanged.

## Concurrent phone preflight finding
The combined RTP-clock/receipt-clock candidate (994c817 +71e55cf) was prepared but NOT deployed in this continuation. Extended preflight observed a missing ping reply, then direct inspection showed CloudGime was no longer foreground and UID10279 had `blocked=APP_BACKGROUND, allowed=NONE, effective=APP_BACKGROUND`; ICMP sends failed `Operation not permitted`, followed by DNS failure. This is observed OS policy, not proof of media packet loss or a hardware defect. User was asked to leave the app open for a bounded physical test. No network/ADB/security/battery settings changed, no emulator, no new rental.

Before physical continuation, the runner must confirm CloudGime remains foreground. Do not repeatedly reopen over the user's other apps or classify explicit background permission denial as network packet loss. Pending RTP physical acceptance and Sentinel channel-exit root cause remain unresolved.

Evidence: workspace `06 TEST/20261001-final-clock-validation`, cache tests/build logs, `preflight.json`, `network-paths.json`. Existing production branch was not pushed or deployed by this change.
